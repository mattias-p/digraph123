extern crate clap;
extern crate cpal;
extern crate rand;
extern crate regex;
extern crate vorbis;

#[macro_use]
extern crate lazy_static;

mod digraph;
mod stream;

use std::error::Error;
use std::env;
use std::fs;
use std::io;
use std::io::Write;
use std::ops::DerefMut;
use std::path;
use std::process;
use std::thread;
use std::time;
use stream::Stream;

type VoiceConfig = (u8, u32);

macro_rules! print_error {
    ($err:expr, $fmt:tt $(, $arg:expr)*) => {{
        writeln!(&mut io::stderr(), concat!("{}: error: ", $fmt, ": {}"), get_prog_name() $(, $arg)*, $err.description()).ok();
        let err = $err;
        while let Some(err) = err.cause() {
            writeln!(&mut io::stderr(), "\tcaused by: {}", err.description()).unwrap();
        }
    }}
}

macro_rules! insist {
    ($res:expr, $fmt:tt $(, $arg:expr)*) => {
        match $res {
            Ok(value) => value,
            Err(ref err) => {
                print_error!(err, $fmt $(, $arg)*);
                process::exit(1);
            }
        }
    }   
}

fn get_prog_name() -> &'static str {
    fn aux() -> String {
        let prog_name = env::args().next().expect("getting the program name");
        path::Path::new(&prog_name)
            .file_name()
            .expect("file_name")
            .to_string_lossy()
            .into_owned()
    }
    lazy_static! {
        static ref PROG_NAME: String = aux();
    }
    PROG_NAME.as_str()
}

fn path_to_section(path: &path::Path) -> Option<(String, String, Option<String>)> {
    lazy_static! {
        static ref SECTION_RE: regex::Regex = regex::Regex::new(r"^([^-]+)-([^-]+)(?:-(.+))?.ogg$").unwrap();
    }
    path.file_name()
        .and_then(|os_str| os_str.to_str())
        .and_then(|file_name| SECTION_RE.captures(file_name))
        .map(|cap| {
            (cap[1].to_lowercase().to_string(),
             cap[2].to_lowercase().to_string(),
             cap.at(3).map(|s| s.to_string()))
        })
}

fn path_to_voice_config(path: &path::Path) -> Result<VoiceConfig, stream::Error> {
    let file = try!(fs::File::open(path));
    let mut decoder = try!(vorbis::Decoder::new(file));
    let packet = try!(decoder.packets().next().expect("first packet"));
    Ok((packet.channels as u8, packet.rate as u32))
}

fn build_player(dir: &str) -> stream::Result<(Option<VoiceConfig>, stream::Player)> {
    let mut voice_config = None;
    let dir_files = try!(fs::read_dir(dir));
    let mut digraph_builder = digraph::DigraphBuilder::new();
    for entry in dir_files {
        let entry = insist!(entry, "traversing directory '{}'", dir);
        let path = entry.path();
        let path_display = path.display();
        if let Some((tail, head, _)) = path_to_section(&path) {
            let file_voice_config = insist!(path_to_voice_config(&path),
                                            "getting voice config of '{}'",
                                            path_display);
            let file_voice_config = Some(file_voice_config);
            voice_config = voice_config.or(file_voice_config);
            if file_voice_config == voice_config {
                digraph_builder = digraph_builder.arrow(tail, head, path.clone());
            } else {
                writeln!(&mut io::stderr(),
                         "{}: warning: incompatible voice config in file '{}'",
                         get_prog_name(),
                         path_display)
                    .ok();
            }
        }
    }
    let digraph: digraph::Digraph = digraph_builder.into();
    let tracks = digraph.into_random_walk(Box::new(rand::thread_rng()))
                        .map(|p| stream::Track::vorbis(p.as_path()));
    Ok((voice_config, stream::Player::new(Box::new(tracks)).unwrap()))
}

fn build_mixer(dirs: &[&str]) -> (VoiceConfig, stream::Mixer, f32) {
    assert!(dirs.len() > 0);
    let mut voice_config = None;
    let mut streams: Vec<Box<stream::Stream>> = vec![];
    for dir in dirs {
        let (dir_voice_config, player) = insist!(build_player(dir),
                                                 "building player for directory '{}'",
                                                 dir);
        voice_config = voice_config.or(dir_voice_config);
        if dir_voice_config == voice_config {
            streams.push(Box::new(player));
        } else {
            writeln!(&mut io::stderr(),
                     "{}: warning: incompatible voice config in directory '{}'",
                     get_prog_name(),
                     dir)
                .ok();
        }
    }

    let coefficient = 1.0 / streams.len() as f32;

    (voice_config.unwrap(),
     stream::Mixer::new(streams),
     coefficient)
}

fn create_voice(voice_config: VoiceConfig, endpoint: cpal::Endpoint) -> cpal::Voice {
    let format = {
        let formats = endpoint.get_supported_formats_list();
        let formats = insist!(formats,
                              "getting list of formats supported by default endpoint");

        formats.filter(|f| f.samples_rate.0 as u32 == voice_config.1)
               .filter(|f| f.channels.len() == voice_config.0 as usize)
               .filter(|f| f.data_type == cpal::SampleFormat::F32)
               .next()
    };
    let format = if let Some(format) = format {
        format
    } else {
        panic!("voice format not supported");
    };

    cpal::Voice::new(&endpoint, &format).expect("Failed to create a voice")
}

fn main() {
    get_prog_name();

    let matches = clap::App::new("digraph123")
                      .version("1.0.0")
                      .author("Mattias Päivärinta")
                      .about("Play digraph shaped audio recordings using random walk")
                      .arg(clap::Arg::with_name("dir")
                               .help("A digraph directory")
                               .index(1)
                               .required(true)
                               .multiple(true))
                      .get_matches();

    let dirs = matches.values_of("dir").map(|v| v.collect()).unwrap_or(vec![]);
    let (voice_config, mut mixer, coefficient) = build_mixer(dirs.as_slice());
    let num_channels = voice_config.0 as usize;

    let endpoint = cpal::get_default_endpoint().expect("default endpoing");
    let mut voice = create_voice(voice_config, endpoint);

    while !mixer.is_eos() {
        let max_read = mixer.max_read();
        assert_eq!(max_read % num_channels, 0);

        if max_read == 0 {
            if let Err(err) = mixer.load() {
                print_error!(err, "loading mixer");
            }
            continue;
        }

        match voice.append_data(max_read) {
            cpal::UnknownTypeBuffer::F32(mut buffer) => {
                for out in buffer.deref_mut().iter_mut() {
                    *out = 0.0;
                }

                mixer.read_add(buffer.deref_mut());

                for out in buffer.deref_mut().iter_mut() {
                    *out *= coefficient;
                }
            }

            cpal::UnknownTypeBuffer::U16(_) => {
                panic!("unsupported buffer type");
            }

            cpal::UnknownTypeBuffer::I16(_) => {
                panic!("unsupported buffer type");
            }
        };

        voice.play();
    }

    while voice.get_pending_samples() > 0 {
        thread::sleep(time::Duration::from_millis(100));
    }
}
