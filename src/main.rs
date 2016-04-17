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
        let mut err = $err as &std::error::Error;
        writeln!(&mut io::stderr(), concat!("{}: ", $fmt, "\n\tcaused by: {}"), get_prog_name() $(, $arg)*, err).ok();
        while let Some(cause) = err.cause() {
            writeln!(&mut io::stderr(), "\tcaused by: {}", cause).ok();
            err = cause;
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

struct PlayerBuilder {
    digraph_builder: digraph::DigraphBuilder,
    voice_config: Option<VoiceConfig>,
}

impl PlayerBuilder {
    fn new() -> PlayerBuilder {
        PlayerBuilder {
            digraph_builder: digraph::DigraphBuilder::new(),
            voice_config: None,
        }
    }

    fn path_to_voice_config(path: &path::Path) -> Result<VoiceConfig, stream::Error> {
        let file = try!(fs::File::open(path));
        let mut decoder = try!(vorbis::Decoder::new(file));
        let packet = try!(decoder.packets().next().expect("first packet"));
        Ok((packet.channels as u8, packet.rate as u32))
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

    fn path(&mut self, path: path::PathBuf) -> stream::Result<&mut Self> {
        if let Some((tail, head, _)) = Self::path_to_section(&path) {
            match Self::path_to_voice_config(&path) {
                Ok(file_voice_config) => {
                    self.voice_config = self.voice_config.or(Some(file_voice_config));
                    if Some(file_voice_config) != self.voice_config {
                        return Err(stream::Error::AudioFormat);
                    }
                    self.digraph_builder.arrow(tail, head, path);
                    Ok(self)
                }
                Err(err) => Err(stream::Error::File(path, Box::new(err))),
            }
        } else {
            Ok(self)
        }
    }

    fn get_voice_config(&self) -> Option<VoiceConfig> {
        self.voice_config
    }

    fn build(self) -> stream::Result<stream::Player> {
        let digraph: digraph::Digraph = self.digraph_builder.into();
        let tracks = digraph.into_random_walk(Box::new(rand::thread_rng()))
                            .map(|p| stream::Track::vorbis(p.as_path()));
        stream::Player::new(Box::new(tracks))
    }
}

struct MixerBuilder {
    streams: Vec<Box<stream::Stream>>,
    voice_config: Option<VoiceConfig>,
}

impl MixerBuilder {
    fn new() -> MixerBuilder {
        MixerBuilder {
            streams: vec![],
            voice_config: None,
        }
    }

    fn dir(&mut self, dir: &str) -> stream::Result<&mut Self> {
        fn inner(this: &mut MixerBuilder, dir: &str) -> stream::Result<()> {
            let mut player_builder = PlayerBuilder::new();
            for entry in try!(fs::read_dir(dir)) {
                let entry = try!(entry);
                if let Err(err) = player_builder.path(entry.path()) {
                    print_error!(&err, "warning: ignoring file due to error");
                }
            }
            let dir_voice_config = player_builder.get_voice_config();
            let player = try!(player_builder.build());

            this.voice_config = this.voice_config.or(dir_voice_config);
            if dir_voice_config == this.voice_config {
                this.streams.push(Box::new(player));
                Ok(())
            } else {
                Err(stream::Error::AudioFormat)
            }
        }
        inner(self, dir)
            .map_err(|err| stream::Error::Dir(dir.to_string(), Box::new(err)))
            .and(Ok(self))
    }

    fn build(self) -> stream::Result<(VoiceConfig, f32, stream::Mixer)> {
        if let Some(voice_config) = self.voice_config {
            let coefficient = 1.0 / self.streams.len() as f32;
            Ok((voice_config, coefficient, stream::Mixer::new(self.streams)))
        } else {
            Err(stream::Error::AudioFormat)
        }
    }
}

fn build_mixer(dirs: &[&str]) -> stream::Result<(VoiceConfig, f32, stream::Mixer)> {
    assert!(dirs.len() > 0);
    let mut mixer_builder = MixerBuilder::new();
    for dir in dirs {
        try!(mixer_builder.dir(dir));
    }
    mixer_builder.build()
}

fn create_voice(voice_config: VoiceConfig, endpoint: cpal::Endpoint) -> cpal::Voice {
    let format = {
        let formats = endpoint.get_supported_formats_list();
        let formats = insist!(formats,
                              "failed to get list of formats supported by default endpoint");

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
                               .multiple(true))
                      .get_matches();

    let dirs = matches.values_of("dir").map(|v| v.collect()).unwrap_or(vec![]);
    let (voice_config, coefficient, mut mixer) = insist!(build_mixer(dirs.as_slice()),
                                                         "failed to construct mixer");
    let num_channels = voice_config.0 as usize;

    let endpoint = cpal::get_default_endpoint().expect("default endpoing");
    let mut voice = create_voice(voice_config, endpoint);

    while !mixer.is_eos() {
        let max_read = mixer.max_read();
        assert_eq!(max_read % num_channels, 0);

        if max_read == 0 {
            if let Err(err) = mixer.load() {
                print_error!(&err, "warning: an error occurred loading the mixer");
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
