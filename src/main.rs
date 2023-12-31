#![feature(unix_sigpipe)]

extern crate colored;
extern crate clap;
extern crate flate2;
extern crate core;

mod trimmers;
mod primers;

use std::io;
use flate2::read::MultiGzDecoder;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use clap::Parser;
use flate2::Compression;
use flate2::write::GzEncoder;
use crate::trimmers::{BackTrimmer, FastqTrimmer, FrontBackTrimmer, PolyXTrimmer, PrimerTrimmer, TrimResult};
use log::{debug, info, warn};

/// A constrained use-case fastq trimmer
#[derive(Parser, Debug)]
struct Args {
    /// the first fastq file -- required
    #[arg(long)]
    fastq1: Option<String>,

    /// a second fastq file, for Illumina paired-end reads
    #[arg(long)]
    fastq2: Option<String>,

    /// output fastq file 1
    #[arg(long)]
    out_fastq1: Option<String>,

    /// output fastq file 2 -- if using paired-end reads
    #[arg(long)]
    out_fastq2: Option<String>,

    /// minimum remaining read size after trimming is complete -- reads shorter than this will be discarded
    #[arg(long, default_value_t = 10)]
    minimum_remaining_read_size: usize,

    /// the minimum average quality score a window of nucleotides must have
    #[arg(long, default_value_t = 10)]
    window_min_qual_score: u8,

    /// trimming window size
    #[arg(long, default_value_t = 10)]
    window_size: u8,

    /// enable poly-A tail trimming (seen in RNA-seq data)
    #[arg(long, default_value_t = false)]
    trim_poly_a: bool,

    /// trim a read after a poly-G tail is found (seen when sequencing off the end of an Illumina read with 2-color chemistry)
    #[arg(long, default_value_t = false)]
    trim_poly_g: bool,

    /// the length of the poly-X tail to trim (use with the poly-A or poly-G trimming)
    #[arg(long, default_value_t = 10)]
    trim_poly_x_length: usize,

    /// the proportion of bases that must be X to trim the read end
    #[arg(long, default_value_t = 0.9)]
    trim_poly_x_proportion: f64,

    /// primers to detect and remove (we'll make their reverse complement too), separated by commas
    #[arg(long)]
    primers: Option<String>,

    /// the maximum mismatches allowed for primer trimming -- it's best if this is 1 or 2
    #[arg(long, default_value_t = 1)]
    primers_max_mismatch_distance: u8,

    /// what proportion of the read ends can a primer be found in (front or back) -- if it's interior to this margin we drop the read(s)
    #[arg(long, default_value_t = 0.2)]
    primers_end_proportion: f64,

    /// Should we split a read into two when we find an internal primer? otherwise we just drop the read when we see an internal primer
    #[arg(long, default_value_t = false)]
    split_on_internal_primers: bool,

    /// just display the reads and what we'd cut, don't actually write any output to disk
    #[arg(long, default_value_t = false)]
    preview: bool,
}

/// a simple FASTQ record with name, sequence, and quality
pub struct FastqRecord {
    pub name: Vec<u8>,
    pub seq: Vec<u8>,
    pub quals: Vec<u8>,
}

impl FastqRecord {
    pub fn new(name: Vec<u8>, seq: Vec<u8>, quals: Vec<u8>) -> FastqRecord {
        FastqRecord { name, seq, quals }
    }
}

/// an input decoder for our gzipped FASTQ file
struct FastqInputFile {
    decoder: BufReader<MultiGzDecoder<File>>,
}

impl FastqInputFile {
    pub fn new(path: &str) -> Result<FastqInputFile, io::Error> {
        let file = File::open(path)?;
        let decoder = io::BufReader::new(MultiGzDecoder::new(file));
        Ok(FastqInputFile { decoder })
    }
}

impl Iterator for FastqInputFile {
    type Item = FastqRecord;

    fn next(&mut self) -> Option<FastqRecord> {
        let mut name = String::new();
        match self.decoder.read_line(&mut name) {
            Ok(_) => {
            }
            Err(_e) => {
                warn!("Error reading sequence line for unnamed read");
                return None
            },
        }
        let mut name = name.into_bytes();
        if name.len() == 0 {
            return None;

        }
        assert_eq!(name[0], b'@');
        name.pop(); // drop endline

        let mut seq = String::new();
        match self.decoder.read_line(&mut seq) {
            Ok(_) => {}
            Err(_e) => {
                warn!("Error reading sequence line for read {}", String::from_utf8(name).unwrap());
                return None
            },
        }
        seq.pop(); // drop endline
        let mut _orient = String::new();
        match self.decoder.read_line(&mut _orient) {
            Ok(_) => {}
            Err(_e) => {
                warn!("Error reading orientation line for read {}", String::from_utf8(name).unwrap());
                return None
            },
        }
        let mut quals = String::new();
        match self.decoder.read_line(&mut quals) {
            Ok(_) => {}
            Err(_e) => {
                warn!("Error reading quals line for read {}", String::from_utf8(name).unwrap());
                return None
            },
        }
        quals.pop(); // drop endline

        Some(FastqRecord { name, seq: seq.into_bytes(), quals: quals.into_bytes() })
    }
}

#[unix_sigpipe = "sig_dfl"]
fn main() {
    simple_logger::init_with_level(log::Level::Warn).unwrap();

    let args = Args::parse();

    assert!(args.preview ^ args.out_fastq1.is_some(), "Either preview mode or output files need to be set");

    let mut out_fastq1 = setup_compressed_file(&args.out_fastq1);
    let mut out_fastq2 = setup_compressed_file(&args.out_fastq2);

    let mut reader = FastqInputFile::new(&args.fastq1.unwrap()).expect("invalid path/file for fastq1");

    let mut cutters: Vec<Box<dyn FastqTrimmer>> = Vec::new();

    if args.primers.is_some() {
        let primers = args.primers.unwrap();
        let primers: Vec<Vec<u8>> = primers.split(",").map(|i|i.as_bytes().to_vec()).collect();
        info!("Using primers: {}", &primers.clone().into_iter().map(|i|String::from_utf8(i).unwrap()).collect::<Vec<String>>().join(", "));

        cutters.push(Box::new(PrimerTrimmer::new(&primers,
                                                 &args.primers_max_mismatch_distance,
                                                 &args.primers_end_proportion,
                                                 &args.split_on_internal_primers)));
    }

    if args.trim_poly_a {
        cutters.push(Box::new(PolyXTrimmer {
            window_size: args.trim_poly_x_length.clone(),
            minimum_g_proportion: args.trim_poly_x_proportion.clone(),
            bases: vec![b'A', b'a'],
        }));
    }
    if args.trim_poly_g {
        cutters.push(Box::new(PolyXTrimmer {
            window_size: args.trim_poly_x_length.clone(),
            minimum_g_proportion: args.trim_poly_x_proportion.clone(),
            bases: vec![b'G', b'g'],
        }));
    }

    if args.fastq2.is_some() {
        cutters.push(Box::new(BackTrimmer { window_size: args.window_size.clone(), window_min_qual_score: args.window_min_qual_score, qual_score_base: 32 }));
        let mut reader2 = FastqInputFile::new(&args.fastq2.unwrap()).expect("invalid path/file for fastq2");
        paired_end(&mut reader, &mut reader2, &mut out_fastq1, &mut out_fastq2, &cutters, &args.minimum_remaining_read_size, &args.preview);
    } else {
        cutters.push(Box::new(FrontBackTrimmer { window_size: args.window_size.clone(), window_min_qual_score: args.window_min_qual_score, qual_score_base: 32 }));
        single_end(&mut reader, &mut out_fastq1, &cutters, &args.minimum_remaining_read_size, &args.preview);
    }
}

fn single_end(reader1: &mut FastqInputFile,
              out_fastq: &mut BufWriter<GzEncoder<Box<dyn Write>>>,
              cutters: &Vec<Box<dyn FastqTrimmer>>,
              minimum_remaining_read_size: &usize,
              preview: &bool) {

    while let Some(read1) = reader1.next() {
        let mut base_cuts = TrimResult::from_read(&read1);
        for cutter in cutters {
            let cut = cutter.trim(&read1);
            debug!("cut: {:?}", cut);
            base_cuts = TrimResult::join(vec![base_cuts, cut], &true);
            debug!("base_cuts: {:?}", base_cuts);
        }

        debug!("base cuts: {:?}", base_cuts);
        if base_cuts.keep() {
            for resulting_read in base_cuts.trim_results_to_reads(&read1) {
                if resulting_read.seq.len() >= *minimum_remaining_read_size {
                    match *preview {
                        true => {
                            print_read(&read1, &base_cuts);
                        }
                        false => {
                            write_read(out_fastq, &resulting_read).expect("Unable to write to output file 1.");
                        }
                    }
                }
            }
        }
    }
    out_fastq.flush().expect("Unable to flush output fastq file.");
}

fn paired_end(reader1: &mut FastqInputFile,
              reader2: &mut FastqInputFile,
              out_fastq1: &mut BufWriter<GzEncoder<Box<dyn Write>>>,
              out_fastq2: &mut BufWriter<GzEncoder<Box<dyn Write>>>,
              cutters: &Vec<Box<dyn FastqTrimmer>>,
              minimum_remaining_read_size: &usize,
              preview: &bool) {

    while let Some(read1) = reader1.next() {
        let read2 = match reader2.next() {
            None => {panic!("Reads in fastq1 and fastq2 are not paired, at read1 {}",String::from_utf8(read1.name).unwrap())}
            Some(x) => {x}
        };

        let mut base_cuts_read1 = TrimResult::from_read(&read1);
        let mut base_cuts_read2 = TrimResult::from_read(&read2);

        for cutter in cutters {
            let cut = cutter.trim(&read1);
            base_cuts_read1 = TrimResult::join(vec![base_cuts_read1, cut], &true);

            let cut = cutter.trim(&read2);
            base_cuts_read2 = TrimResult::join(vec![base_cuts_read2, cut], &true);
        }

        let resulting_reads1 = base_cuts_read1.trim_results_to_reads(&read1);
        let resulting_reads2 = base_cuts_read2.trim_results_to_reads(&read2);
        assert_eq!(resulting_reads1.len(), resulting_reads2.len(),"{}", format!("Resulting read split from read1: {} and read2: {} are not the same segment lengths ({} and {})",
                                                                          String::from_utf8(read1.name).unwrap(),
                                                                          String::from_utf8(read2.name).unwrap(),
                                                                          resulting_reads1.len(),resulting_reads2.len()));

        for read_index in 0..resulting_reads1.len() {
            let read1 = &resulting_reads1[read_index];
            let read2 = &resulting_reads2[read_index];

            if read1.seq.len() >= *minimum_remaining_read_size && read2.seq.len() >= *minimum_remaining_read_size {
                match *preview {
                    true => {
                        print_read(read1, &base_cuts_read1);
                        print_read(read2, &base_cuts_read2);
                    }
                    false => {
                        write_read(out_fastq1, read1).expect("Unable to write to output file 1.");
                        write_read(out_fastq2, read2).expect("Unable to write to output file 2.");
                    }
                }
            }
        }
    }
    out_fastq1.flush().expect("Unable to flush output file 1.");
    out_fastq2.flush().expect("Unable to flush output file 2.");
}

fn setup_compressed_file(fastq_output: &Option<String>) -> BufWriter<GzEncoder<Box<dyn Write>>> {
    let writer1: Box<dyn Write> = match fastq_output.clone() {
        Some(file) => Box::new(File::create(file).unwrap()),
        None => Box::new(io::stdout()),
    };

    let out_fastq1 = BufWriter::new(GzEncoder::new(writer1, Compression::default()));
    out_fastq1
}

pub fn write_read(writer: &mut BufWriter<dyn Write>, record: &FastqRecord) -> Result<(), io::Error> {
    writer.write_all(&record.name)?;
    writer.write_all(b"\n")?;
    writer.write_all(&record.seq)?;
    writer.write_all(b"\n+\n")?;
    writer.write_all(&record.quals)?;
    writer.write_all(b"\n")?;
    Ok(())
}

pub fn print_read(read: &FastqRecord, trim_result: &TrimResult) {
    println!(">{}\n{}", String::from_utf8(read.name.to_vec()).unwrap(),trim_result.print_format_read(read));

}


pub fn color_qual_proportion(quality: &u32) -> u8 {
    assert!(quality >= &33, "Quality score must be at least 33. {} ", quality);
    ((((if quality > &93 { 93 } else { quality.clone() }) - 33) as f64 / (93 - 33) as f64) * 255.0) as u8
}