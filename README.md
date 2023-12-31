# Butcher

Butcher is designed to trim reads for single-end long-read sequencing (like Nanopore) and paired-end Illumina sequencing. Similar to other tools such as Nanofilt, Porechop, and Trimmomatic, Butcher excels in trimming low-quality regions from the ends of reads, detecting and removing adapter sequences, and eradicating poly-A and poly-G tracks from sequencing reads. A standout feature of Butcher is its ability to provide a preview of the changes directly in the terminal. This lets users see the expected results before they decide to proceed with the operation.

![butcher annimation](https://github.com/mckennalab/butcher/blob/22f80955e85121a8a70da0585c7d2f4115fad3d2/render1696614684110.gif)

## Getting butcher

Downloads are available on the release page for Linux systems, or you can build it (see below).

## Documentation

**You want to preserve color annotations** in piped processes, and you'll need to set the command line coloring option. For bash this looks something like ```export CLICOLOR_FORCE=1```. This will then color the output when previewing your cut reads.

```
A constrained use-case fastq trimmer

Usage: butcher [OPTIONS]

Options:
      --fastq1 <FASTQ1>
          the first fastq file -- required
      --fastq2 <FASTQ2>
          a second fastq file, for Illumina paired-end reads
      --out-fastq1 <OUT_FASTQ1>
          output fastq file 1
      --out-fastq2 <OUT_FASTQ2>
          output fastq file 2 -- if using paired-end reads
      --minimum-remaining-read-size <MINIMUM_REMAINING_READ_SIZE>
          minimum remaining read size after trimming is complete -- reads shorter than this will be discarded [default: 10]
      --window-min-qual-score <WINDOW_MIN_QUAL_SCORE>
          the minimum average quality score a window of nucleotides must have [default: 10]
      --window-size <WINDOW_SIZE>
          trimming window size [default: 10]
      --trim-poly-a
          enable poly-A tail trimming (seen in RNA-seq data)
      --trim-poly-g
          trim a read after a poly-G tail is found (seen when sequencing off the end of an Illumina read with 2-color chemistry)
      --trim-poly-x-length <TRIM_POLY_X_LENGTH>
          the length of the poly-X tail to trim (use with the poly-A or poly-G trimming) [default: 10]
      --trim-poly-x-proportion <TRIM_POLY_X_PROPORTION>
          the proportion of bases that must be X to trim the read end [default: 0.9]
      --primers <PRIMERS>
          primers to detect and remove (we'll make their reverse complement too), separated by commas
      --primers-max-mismatch-distance <PRIMERS_MAX_MISMATCH_DISTANCE>
          the maximum mismatches allowed for primer trimming -- it's best if this is 1 or 2 [default: 1]
      --primers-end-proportion <PRIMERS_END_PROPORTION>
          what proportion of the read ends can a primer be found in (front or back) -- if it's interior to this margin we drop the read(s) [default: 0.2]
      --preview
          just display the reads and what we'd cut, don't actually write any output to disk
  -h, --help
          Print help

```

## Compiling

Rust nightly is required to avoid the crash text when you pipe (```|```) __butcher__ on the command line. Building with nightly is simple:
```
cargo build --release
```
From the command line. A __butcher__ artifact will be created in the target/release/ folder.

