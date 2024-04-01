use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use ffmpeg::color::TransferCharacteristic;
use ffmpeg::format::{input, Pixel};
use ffmpeg::media::{self, Type as MediaType};
use ffmpeg::ChannelLayout;
use ffmpeg::Error::StreamNotFound;
use path_abs::{PathAbs, PathInfo};

use crate::{into_array, into_vec};

pub fn compose_ffmpeg_pipe<S: Into<String>>(
  params: impl IntoIterator<Item = S>,
  pix_format: Pixel,
) -> Vec<String> {
  let mut p: Vec<String> = into_vec![
    "ffmpeg",
    "-y",
    "-hide_banner",
    "-loglevel",
    "error",
    "-i",
    "-",
  ];

  p.extend(params.into_iter().map(Into::into));

  p.extend(into_array![
    "-pix_fmt",
    pix_format.descriptor().unwrap().name(),
    "-strict",
    "-1",
    "-f",
    "yuv4mpegpipe",
    "-"
  ]);

  p
}

/// Get frame count using FFmpeg
pub fn num_frames(source: &Path) -> Result<usize, ffmpeg::Error> {
  let mut ictx = input(&source)?;
  let input = ictx
    .streams()
    .best(MediaType::Video)
    .ok_or(StreamNotFound)?;
  let video_stream_index = input.index();

  Ok(
    ictx
      .packets()
      .filter(|(stream, _)| stream.index() == video_stream_index)
      .count(),
  )
}

pub fn frame_rate(source: &Path) -> Result<f64, ffmpeg::Error> {
  let ictx = input(&source)?;
  let input = ictx
    .streams()
    .best(MediaType::Video)
    .ok_or(StreamNotFound)?;
  let rate = input.avg_frame_rate();
  Ok(f64::from(rate.numerator()) / f64::from(rate.denominator()))
}

pub fn get_pixel_format(source: &Path) -> Result<Pixel, ffmpeg::Error> {
  let ictx = ffmpeg::format::input(&source)?;

  let input = ictx
    .streams()
    .best(MediaType::Video)
    .ok_or(StreamNotFound)?;

  let decoder = ffmpeg::codec::context::Context::from_parameters(input.parameters())?
    .decoder()
    .video()?;

  Ok(decoder.format())
}

pub fn resolution(source: &Path) -> Result<(u32, u32), ffmpeg::Error> {
  let ictx = ffmpeg::format::input(&source)?;

  let input = ictx
    .streams()
    .best(MediaType::Video)
    .ok_or(StreamNotFound)?;

  let decoder = ffmpeg::codec::context::Context::from_parameters(input.parameters())?
    .decoder()
    .video()?;

  Ok((decoder.width(), decoder.height()))
}

pub fn transfer_characteristics(source: &Path) -> Result<TransferCharacteristic, ffmpeg::Error> {
  let ictx = ffmpeg::format::input(&source)?;

  let input = ictx
    .streams()
    .best(MediaType::Video)
    .ok_or(StreamNotFound)?;

  let decoder = ffmpeg::codec::context::Context::from_parameters(input.parameters())?
    .decoder()
    .video()?;

  Ok(decoder.color_transfer_characteristic())
}

/// Returns vec of all keyframes
pub fn get_keyframes(source: &Path) -> Result<Vec<usize>, ffmpeg::Error> {
  let mut ictx = input(&source)?;
  let input = ictx
    .streams()
    .best(MediaType::Video)
    .ok_or(StreamNotFound)?;
  let video_stream_index = input.index();

  let kfs = ictx
    .packets()
    .filter(|(stream, _)| stream.index() == video_stream_index)
    .map(|(_, packet)| packet)
    .enumerate()
    .filter(|(_, packet)| packet.is_key())
    .map(|(i, _)| i)
    .collect::<Vec<_>>();

  if kfs.is_empty() {
    return Ok(vec![0]);
  };

  Ok(kfs)
}

/// Returns true if input file have audio in it
pub fn has_audio(file: &Path) -> bool {
  let ictx = input(&file).unwrap();
  ictx.streams().best(MediaType::Audio).is_some()
}

pub fn get_channel_layout_float(stream: &ffmpeg::Stream<'_>) -> f32 {
  let layout_bits: u64 = unsafe { (*stream.parameters().as_ptr()).ch_layout.u.mask };
  let channels: i32 = unsafe { (*stream.parameters().as_ptr()).ch_layout.nb_channels };

  match ChannelLayout::from_bits(layout_bits) {
    Some(layout) => {
      return match layout {
        ChannelLayout::_2POINT1 | ChannelLayout::_2_1 => 2.1,
        ChannelLayout::_2_2 => 2.2,
        ChannelLayout::_3POINT1 => 3.1,
        ChannelLayout::_4POINT1 => 4.1,
        ChannelLayout::_5POINT1 | ChannelLayout::_5POINT1_BACK => 5.1,
        ChannelLayout::_6POINT1 | ChannelLayout::_6POINT1_FRONT | ChannelLayout::_6POINT1_BACK => 6.1,
        ChannelLayout::_7POINT1 | ChannelLayout::_7POINT1_WIDE | ChannelLayout::_7POINT1_WIDE_BACK => 7.1,
        _ => channels as f32
      };
    },
    None => {
      return match channels {
        3 => 2.1,
        6 => 5.1,
        8 => 7.1,
        _ => channels as f32
      };
    },
  }
}

pub fn handle_opus(input: &Path, merge_with: &Path, output: &Path, temp: &Path) {
  let ictx = ffmpeg::format::input(&input).unwrap();

  if !temp.join("audio").exists() {
    std::fs::create_dir(temp.join("audio")).expect("Failed to create audio folder");
  }

  let audio_data = ictx
    .streams()
    .filter(|f| f.parameters().medium() == media::Type::Audio)
    .fold(Vec::new(), |mut vec, stream| {
      let layout = get_channel_layout_float(&stream);
      let bitrate = (128.0 * (layout / 2.0).powf(0.75)).round() as usize;

      let ffmpeg = Command::new("ffmpeg")
        .args(["-hide_banner", "-v", "quiet", "-i"])
        .arg(input.to_str().unwrap())
        .args(["-vn", "-sn", "-dn", "-map"])
        .arg(format!("0:{}", stream.index()))
        .args(["-map_metadata".to_owned(), format!("0:s:{}", stream.index())])
        .args(["-f", "flac", "-"])
        .stdout(Stdio::piped())
        .spawn()
        .expect("ffmpeg failed to start");

      let mut opusenc = Command::new("opusenc")
        .args(["--quiet", "--vbr", "--bitrate"])
        .arg(format!("{bitrate}K"))
        .arg("-")
        .arg(format!("{}/audio/{}.opus", temp.to_string_lossy(), stream.index()))
        .stdin(Stdio::from(ffmpeg.stdout.unwrap()))
        .spawn()
        .expect("opusenc failed to start");

      opusenc.wait().expect("Opusenc crashed");

      vec.push(
        stream.index()
      );

      vec
    });

  let (input_args, map_args, map_counter) = audio_data
    .iter()
    .fold((Vec::new(), Vec::new(), 0usize), |(mut input_args, mut map_args, mut c), a| {
      input_args.push(format!("-i"));
      input_args.push(format!("{}/audio/{}.opus", temp.to_string_lossy(), a));
      map_args.push(format!("-map"));
      map_args.push(format!("{}", c));
      c += 1;
      (input_args, map_args, c)
    });

  let mut ffmpeg_merge = Command::new("ffmpeg")
    .args(["-y", "-hide_banner", "-v", "quiet"])
    .args(&input_args)
    .arg("-i")
    .arg(merge_with.to_str().unwrap())
    .args(["-map 0:s"]) // Only map the subtitle streams
    .args(&map_args)
    .args(["-map".to_owned(), format!("{}", map_counter)])
    .args(["-c", "copy"])
    .arg(output.to_str().unwrap())
    .spawn()
    .expect("ffmpeg failed to start");

  ffmpeg_merge.wait().expect("ffmpeg crashed while merging");
}

/// Encodes the audio using FFmpeg, blocking the current thread.
///
/// This function returns `Some(output)` if the audio exists and the audio
/// successfully encoded, or `None` otherwise.
#[must_use]
pub fn encode_audio<S: AsRef<OsStr>>(
  input: impl AsRef<Path>,
  temp: impl AsRef<Path>,
  opus_mode: bool,
  audio_params: &[S],
) -> Option<PathBuf> {
  let input = input.as_ref();
  let temp = temp.as_ref();

  if has_audio(input) {
    let audio_file = match opus_mode {
        true => Path::new(temp).join("misc.mkv"),
        false => Path::new(temp).join("audio.mkv"),
    };
    let mut encode_audio = Command::new("ffmpeg");

    encode_audio.stdout(Stdio::piped());
    encode_audio.stderr(Stdio::piped());

    encode_audio.args(["-y", "-hide_banner", "-loglevel", "error", "-i"]);
    encode_audio.arg(input);

    encode_audio.args([
      "-map_metadata",
      "0",
      "-vn",
      "-dn",
      "-map",
      "0",
      "-c",
      "copy"
    ]);

    match opus_mode {
        true => {encode_audio.args(["-map", "0:a:0"])}, // We need one audio track to keep the subtitles in sync.
        false => encode_audio.args(audio_params),
    };
    
    encode_audio.arg(&audio_file);

    let output = encode_audio.output().unwrap();

    if !output.status.success() {
      warn!(
        "FFmpeg failed to encode audio!\n{:#?}\nParams: {:?}",
        output, encode_audio
      );
      return None;
    } else if opus_mode {
      handle_opus(
        input, 
        &audio_file, 
        Path::new(temp).join("audio.mkv").as_path(),
        temp
      );

      Some(Path::new(temp).join("audio.mkv"))
    } else {
      Some(audio_file)
    }
  } else {
    None
  }
}

/// Escapes paths in ffmpeg filters if on windows
pub fn escape_path_in_filter(path: impl AsRef<Path>) -> String {
  if cfg!(windows) {
    PathAbs::new(path.as_ref())
      .unwrap()
      .to_str()
      .unwrap()
      // This is needed because of how FFmpeg handles absolute file paths on Windows.
      // https://stackoverflow.com/questions/60440793/how-can-i-use-windows-absolute-paths-with-the-movie-filter-on-ffmpeg
      .replace('\\', "/")
      .replace(':', r"\\:")
  } else {
    PathAbs::new(path.as_ref())
      .unwrap()
      .to_str()
      .unwrap()
      .to_string()
  }
  .replace('[', r"\[")
  .replace(']', r"\]")
  .replace(',', "\\,")
}
