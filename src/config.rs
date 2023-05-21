use serde::Deserialize;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("input/output error: {0}")]
    IO(std::io::Error),
    #[error("cannot parse TOML: {0}")]
    TOMLParse(toml::de::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Deserialize)]
pub struct Srt {
    uri: String,
    streamid: String,
}

impl Srt {
    pub fn gst_sink(&self) -> String {
        format!("srtsink uri={} streamid={}", self.uri, self.streamid)
    }
}

#[derive(Deserialize)]
pub struct File {
    path: String,
}

impl File {
    pub fn gst_sink(&self) -> String {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("hmm, a Rust program cannot run before 1970")
            .as_secs()
            .to_string();
        let path = self.path.replace("%t", &now);
        format!("filesink location={}", path)
    }
}

#[derive(Deserialize)]
pub struct Pulse {
    device: String,
}

impl Pulse {
    pub fn gst_source(&self) -> String {
        format!("pulsesrc device={} buffer-time=2000000", self.device)
    }
}

#[derive(Deserialize)]
pub enum Encoder {
    OpenH264,
    VaH264,
    X264,
}

impl Default for Encoder {
    fn default() -> Self {
        Self::X264
    }
}

impl Encoder {
    fn gst_pipeline(&self, framerate: usize) -> String {
        let gop_size = framerate / 2;

        const H264_FORMAT: &str = concat!(
            "video/x-h264",
            ",",
            "profile=constrained-baseline",
            ",",
            "format=yuv420p",
        );

        match self {
            Self::OpenH264 => {
                [
                    "openh264enc",
                    "usage-type=screen",
                    "complexity=low",
                    "slice-mode=auto",
                    &format!("multi-thread={}", num_cpus::get()),
                    &format!("gop-size={gop_size}"),
                ]
                .join(" ")
                    + " ! "
                    + H264_FORMAT
            }
            Self::X264 => {
                [
                    "x264enc",
                    "speed-preset=superfast",
                    "tune=zerolatency",
                    "byte-stream=true",
                    "sliced-threads=true",
                    &format!("key-int-max={gop_size}"),
                ]
                .join(" ")
                    + " ! "
                    + H264_FORMAT
            }
            Self::VaH264 => {
                ["vah264enc", "target-usage=7", "bitrate=2000"].join(" ") + " ! " + H264_FORMAT
            }
        }
    }
}

#[derive(Deserialize)]
pub struct Video {
    width: usize,
    height: usize,
    framerate: usize,
    #[serde(default)]
    cursor: bool,
    #[serde(default)]
    encoder: Encoder,
}

impl Video {
    pub fn gst_pipeline(&self) -> String {
        let Self {
            framerate,
            width,
            height,
            encoder,
            ..
        } = &self;

        const VIDEO_CONVERT: &str = concat!(
            "videoconvert",
            " ",
            "chroma-mode=GST_VIDEO_CHROMA_MODE_NONE",
            " ",
            "dither=GST_VIDEO_DITHER_NONE",
            " ",
            "matrix-mode=GST_VIDEO_MATRIX_MODE_OUTPUT_ONLY",
            " ",
            "n-threads=1",
        );

        [
            &format!("video/x-raw,max-framerate={framerate}/1"),
            VIDEO_CONVERT,
            "videorate",
            &format!("video/x-raw,framerate={framerate}/1"),
            "videoscale",
            &format!("video/x-raw,width={width},height={height}"),
            &encoder.gst_pipeline(*framerate),
        ]
        .join(" ! ")
    }

    pub fn cursor(&self) -> bool {
        self.cursor
    }
}

#[derive(Default, Deserialize)]
pub struct Audio {
    channel: Option<usize>,
    sample_rate: Option<usize>,
    bit_rate: Option<usize>,
}

impl Audio {
    pub fn gst_pipeline(&self) -> String {
        let channel = self.channel.unwrap_or(2);
        let sample_rate = self.sample_rate.unwrap_or(48000);
        let bit_rate = self.bit_rate.map(|x| format!("bitrate={}", x));

        [
            [Some("fdkaacenc".to_owned()), bit_rate]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" "),
            format!("audio/mpeg,channels={channel},rate={sample_rate}"),
        ]
        .join(" ! ")
    }
}

#[derive(Deserialize)]
pub struct Config {
    pub srt: Option<Srt>,
    pub file: Option<File>,
    pub pulse: Option<Pulse>,
    pub video: Video,
    #[serde(default)]
    pub audio: Audio,
}

impl Config {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = std::fs::read_to_string(path).map_err(Error::IO)?;
        toml::from_str(&content).map_err(Error::TOMLParse)
    }
}
