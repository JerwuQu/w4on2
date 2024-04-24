use std::{fs, path::PathBuf};

use clap::Parser;
use w4on2_shared::*;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
enum Args {
    #[command(about = "Convert a MIDI and TOML combo to w4on2")]
    Convert {
        #[arg(index = 1, help = "Song definition file path")]
        toml: PathBuf,

        #[arg(
            short = 'm',
            long,
            help = "MIDI input file path - defaults to TOML path with .mid extension"
        )]
        midi: Option<PathBuf>,

        #[arg(
            short = 'o',
            long,
            help = "w4on2 output file path - defaults to TOML path with .w4on2 extension"
        )]
        output: Option<PathBuf>,

        #[arg(long, help = "don't stretch to optimal bpm")]
        no_stretch: bool,

        #[arg(long, help = "don't crunch w4on2 file")]
        no_crunch: bool,
    },
    #[command(about = "Convert a w4on2 file to WAV")]
    Bounce {
        #[arg(index = 1, help = "w4on2 input file path")]
        input: PathBuf,

        #[arg(
            short = 'o',
            long,
            help = "WAV output file path - defaults to input path with .wav extension"
        )]
        output: Option<PathBuf>,
    },
}

fn main() {
    let args = Args::parse();

    match args {
        Args::Convert {
            midi,
            toml,
            output,
            no_stretch,
            no_crunch,
        } => {
            let midi_path = midi.unwrap_or(toml.with_extension("mid"));
            let output_path = output.unwrap_or(toml.with_extension("w4on2"));
            let midi_bytes = fs::read(midi_path).expect("failed to load midi file");
            let toml_str = fs::read_to_string(&toml).expect("failed to load toml file");
            let conf = SongConfig::from_toml(&toml_str).expect("failed to parse toml");
            let serialized =
                w4on2_shared::convert::convert(&conf, &midi_bytes, !no_stretch, !no_crunch).expect("failed to convert");
            fs::write(output_path, serialized).expect("failed to write file");
        }
        Args::Bounce { input, output } => {
            let w4on2_bytes = fs::read(&input).expect("failed to load midi file");
            let output_path = output.unwrap_or(input.with_extension("wav"));
            let pcm = w4on2_shared::bounce::bounce_pcm(&w4on2_bytes);
            let mut output_file = std::fs::File::create(output_path).expect("failed to open output file");
            w4on2_shared::bounce::write_wav(pcm, &mut output_file).expect("failed to write output file");
        }
    }
}
