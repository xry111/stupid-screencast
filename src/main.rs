use async_std::stream::StreamExt;
use gstreamer::prelude::*;
use std::collections::HashMap;
use std::error::Error;
use zbus::zvariant::{Array, Structure, Value};

mod config;
mod portal;

use config::Config;
use portal::RequestProxy;

fn check_request_path(a: &RequestProxy, b: &RequestProxy) -> Result<(), &'static str> {
    if a.path() != b.path() {
        return Err("unexpected Request object path");
    }
    Ok(())
}

#[async_std::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = Config::new("config.toml")?;
    if config.file.is_none() && config.srt.is_none() {
        return (Err("no output is configured"))?;
    }

    let bus = zbus::Connection::session().await?;
    let cast = portal::DesktopScreenCastProxy::builder(&bus)
        .build()
        .await?;

    let name = bus
        .unique_name()
        .ok_or("cannot get D-Bus connection unique name")?
        .strip_prefix(':')
        .ok_or("D-Bus connection unique name not prefixed with ':'")?
        .replace('.', "_");

    // Manually constructing this is nasty.  But we don't have a better way
    // because we need to create the signal stream *before* calling
    // create_session() we'll lose signals because of a race.  So we cannot
    // use the return value of create_session() here.
    let req_path = [
        "/org/freedesktop/portal/desktop/request",
        &name,
        "stupid_screencast",
    ]
    .join("/");

    let req = portal::RequestProxy::builder(&bus)
        .path(req_path)?
        .build()
        .await?;

    let mut stream = req.receive_response().await?;

    let opt = HashMap::from([
        ("handle_token", Value::from("stupid_screencast")),
        ("session_handle_token", Value::from("stupid_screencast")),
    ]);
    let request = cast.create_session(&opt).await?;
    check_request_path(&request, &req)?;

    let resp = stream
        .next()
        .await
        .ok_or("no response for CreateSession request")?;
    let args = resp.args()?;

    if args.code() != &0 {
        return (Err("CreateSession returned non-zero code"))?;
    }

    let session_path = args
        .results()
        .get("session_handle")
        .ok_or("no session_handle in CreateSession response")?;
    let session_path = session_path
        .downcast_ref::<str>()
        .ok_or("session_handle is not a string in CreateSession response")?;

    let session = portal::SessionProxy::builder(&bus)
        .path(session_path)?
        .build()
        .await?;

    let opt = HashMap::from([
        ("cursor_mode", Value::from(config.video.cursor() as u32 + 1)),
        ("handle_token", Value::from("stupid_screencast")),
    ]);
    let request = cast.select_sources(&session, &opt).await?;
    check_request_path(&request, &req)?;

    let resp = stream
        .next()
        .await
        .ok_or("no response for SelectSources request")?;
    let args = resp.args()?;

    if args.code() != &0 {
        return (Err("SelectSources returned non-zero code"))?;
    }

    let opt = HashMap::from([("handle_token", Value::from("stupid_screencast"))]);
    let request = cast.start(&session, "", &opt).await?;
    check_request_path(&request, &req)?;

    let resp = stream.next().await.ok_or("no response for Start request")?;
    let args = resp.args()?;

    if args.code() != &0 {
        return (Err("Start returned non-zero code"))?;
    }

    let node_id = &args
        .results()
        .get("streams")
        .ok_or("fuck")?
        .downcast_ref::<Array>()
        .ok_or("fuck")?
        .get()[0]
        .downcast_ref::<Structure>()
        .ok_or("fuck")?
        .fields()[0]
        .downcast_ref::<u32>()
        .ok_or("fuck")?;

    let opt = HashMap::new();
    let fd = cast.open_pipe_wire_remote(&session, &opt).await?;

    let pipe_wire_src = [
        "pipewiresrc",
        &format!("path={}", node_id),
        &format!("fd={}", fd),
        "always-copy=true",
        "do-timestamp=true",
        "keepalive-time=1000",
        "resend-last=true",
    ]
    .join(" ");

    let video_convert = [
        "videoconvert",
        "chroma-mode=GST_VIDEO_CHROMA_MODE_NONE",
        "dither=GST_VIDEO_DITHER_NONE",
        "matrix-mode=GST_VIDEO_MATRIX_MODE_OUTPUT_ONLY",
        "n-threads=1",
    ]
    .join(" ");

    let x264_enc = [
        "x264enc",
        "speed-preset=superfast",
        "tune=zerolatency",
        "byte-stream=true",
        "sliced-threads=true",
    ]
    .join(" ");

    let pipeline_v = [
        &pipe_wire_src,
        &video_convert,
        &config.video.gst_pipeline(),
        &x264_enc,
        "video/x-h264,profile=constrained-baseline,format=yuv420p",
    ]
    .join(" ! ");

    let pipeline_a = config
        .pulse
        .map(|x| [x.gst_source(), config.audio.gst_pipeline()].join(" ! "));

    let srt_sink = config
        .srt
        .as_ref()
        .map(|x| "queue ! ".to_string() + &x.gst_sink());

    let file_sink = config
        .file
        .as_ref()
        .map(|x| "queue ! ".to_string() + &x.gst_sink());

    let sinks = [file_sink, srt_sink]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" tee. ! ");

    let pipeline_out = ["mpegtsmux name=mux", "tee name=tee", &sinks].join(" ! ");

    let pipeline = [Some(pipeline_v), pipeline_a]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ! mux. ")
        + " ! "
        + &pipeline_out;

    println!("{}", pipeline);

    gstreamer::init()?;
    let pipeline = gstreamer::parse_launch(&pipeline)?;

    let bus = pipeline.bus().ok_or("fuck")?;

    pipeline.set_state(gstreamer::State::Playing)?;

    for msg in bus.iter_timed(gstreamer::ClockTime::NONE) {
        use gstreamer::MessageView;

        match msg.view() {
            MessageView::Eos(..) => break,
            MessageView::Error(err) => {
                println!(
                    "Error from {:?}: {} ({:?})",
                    err.src().map(|s| s.path_string()),
                    err.error(),
                    err.debug()
                );
                break;
            }
            _ => (),
        }
    }

    Ok(())
}
