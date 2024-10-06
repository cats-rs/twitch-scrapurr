use anyhow::{Context, Result};
use chrono::Local;
use clap::Parser;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use url::Url;
use vcsr::{args, process_file as vcsr_process_file};
use walkdir::WalkDir;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Twitch username to record
    #[arg(short, long)]
    username: Option<String>,

    /// Custom output directory
    #[arg(short, long)]
    output_dir: Option<String>,

    /// Twitch VOD or Clip URL
    #[arg(short, long)]
    video_url: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Settings {
    output_folder: String,
    convert_to_mp4: bool,
    use_ffmpeg_convert: bool,
    generate_contact_sheet: bool,
    check_interval: u64,
}

struct RecordingState {
    current_file: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let config = load_config()?;

    let output_dir = args.output_dir.unwrap_or(config.output_folder.clone());

    if let Some(video_url) = args.video_url {
        let url = Url::parse(&video_url).context("Invalid URL")?;
        let state = Arc::new(Mutex::new(RecordingState { current_file: None }));
        let state_clone = Arc::clone(&state);

        tokio::select! {
            result = async {
                match url.host_str() {
                    Some("www.twitch.tv") | Some("twitch.tv") => {
                        let path_segments: Vec<&str> = url.path_segments().unwrap().collect();
                        match path_segments.get(0) {
                            Some(&"videos") => process_vod(&video_url, &output_dir, &config, &state).await?,
                            Some(&"clip") | Some(_) if path_segments.contains(&"clip") => {
                                process_clip(&video_url, &output_dir, &config, &state).await?
                            }
                            _ => return Err(anyhow::anyhow!("Invalid Twitch URL")),
                        }
                    }
                    Some("clips.twitch.tv") => process_clip(&video_url, &output_dir, &config, &state).await?,
                    _ => return Err(anyhow::anyhow!("Invalid Twitch URL")),
                }
                Ok(())
            } => {
                if let Err(e) = result {
                    eprintln!("Processing error: {}", e);
                }
            }
            _ = handle_interrupt(state_clone) => {}
        }

        cleanup(&config, &state).await?;
    } else {
        let twitch_username = args.username.unwrap_or_else(|| {
            println!("Streamer Username to record:");
            let mut input = String::new();
            std::io::stdin().read_line(&mut input).unwrap();
            input.trim().to_string()
        });

        let state = Arc::new(Mutex::new(RecordingState { current_file: None }));
        let state_clone = Arc::clone(&state);

        tokio::select! {
            result = record_stream(&twitch_username, &config, &state, &output_dir) => {
                if let Err(e) = result {
                    eprintln!("Recording error: {}", e);
                }
            }
            _ = handle_interrupt(state_clone) => {}
        }

        cleanup(&config, &state).await?;
    }

    Ok(())
}

fn load_config() -> Result<Settings> {
    let project_dirs = ProjectDirs::from("", "", "twitch-scrapurr")
        .context("Failed to get project directories")?;
    let config_dir = project_dirs.config_dir();
    fs::create_dir_all(config_dir)?;
    let config_path = config_dir.join("config.toml");

    let config_str = if config_path.exists() {
        fs::read_to_string(&config_path)?
    } else {
        let default_config = r#"
output_folder = ""
convert_to_mp4 = true
generate_contact_sheet = true
use_ffmpeg_convert = true
check_interval = 60
"#;
        fs::write(&config_path, default_config)?;
        default_config.to_string()
    };

    let mut settings: Settings = toml::from_str(&config_str)?;

    if settings.output_folder.is_empty() {
        print!("Enter the output folder path for recordings: ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        settings.output_folder = input.trim().to_string();

        // Save the updated config
        let updated_config = toml::to_string(&settings)?;
        fs::write(config_path, updated_config)?;
    }

    Ok(settings)
}

async fn handle_interrupt(state: Arc<Mutex<RecordingState>>) {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for ctrl+c");
    println!("Received Ctrl+C, shutting down gracefully ᗜˬᗜ ");
    let state = state.lock().await;
    if let Some(current_file) = &state.current_file {
        println!("Interrupt received. Current file: {:?}", current_file);
    }
}

async fn record_stream(
    username: &str,
    config: &Settings,
    state: &Arc<Mutex<RecordingState>>,
    output_dir: &str,
) -> Result<()> {
    let user_vod_folder = PathBuf::from(&output_dir).join(username).join("vods");
    std::fs::create_dir_all(&user_vod_folder)?;

    loop {
        let stream_url = format!("https://www.twitch.tv/{}", username);
        let output = Command::new("streamlink")
            .args(&["--stream-url", &stream_url, "best"])
            .output()?;

        if output.status.success() {
            println!("Stream is live! Recording {}'s stream.", username);
            let timestamp = Local::now().format("%d_%m_%y-%H_%M").to_string();
            let ts_filename = format!("{}-{}.ts", username, timestamp);
            let ts_filepath = user_vod_folder.join(&ts_filename);

            {
                let mut state = state.lock().await;
                state.current_file = Some(ts_filepath.clone());
            }

            let streamlink_status = Command::new("streamlink")
                .args(&[
                    "--twitch-disable-ads",
                    &stream_url,
                    "best",
                    "-o",
                    ts_filepath.to_str().unwrap(),
                ])
                .status()?;

            if streamlink_status.success() {
                println!("Stream ended. Processing file...");
                process_file(&config, &ts_filepath).await?;
            }

            println!("Waiting briefly before checking for the next stream...");
        } else {
            println!("No available streams found for {}.", username);
            println!(
                "Checking for {} stream again in {} seconds...",
                username, config.check_interval
            );
        }

        sleep(Duration::from_secs(config.check_interval)).await;
    }
}

async fn process_file(config: &Settings, ts_filepath: &PathBuf) -> Result<()> {
    if !ts_filepath.exists() || ts_filepath.metadata()?.len() == 0 {
        println!("File is empty or does not exist. Skipping processing.");
        return Ok(());
    }

    let mp4_filepath = if config.convert_to_mp4 {
        let mp4_filepath = ts_filepath.with_extension("mp4");
        if config.use_ffmpeg_convert {
            let ffmpeg_status = Command::new("ffmpeg")
                .args(&[
                    "-i",
                    ts_filepath.to_str().unwrap(),
                    "-c",
                    "copy",
                    "-y", // Overwrite output file if it exists
                    mp4_filepath.to_str().unwrap(),
                ])
                .status()?;

            if ffmpeg_status.success() {
                std::fs::remove_file(ts_filepath)?;
                println!("[ffmpeg] Converted and saved as: {:?}", mp4_filepath);
                mp4_filepath
            } else {
                println!("[ffmpeg] Conversion failed. Keeping original file.");
                ts_filepath.clone()
            }
        } else {
            std::fs::rename(ts_filepath, &mp4_filepath)?;
            println!("[ffmpeg] Renamed and saved as: {:?}", mp4_filepath);
            mp4_filepath
        }
    } else {
        println!("Saved as: {:?}", ts_filepath);
        ts_filepath.clone()
    };

    if config.generate_contact_sheet {
        if let Err(e) = generate_contact_sheet(&mp4_filepath).await {
            println!("Failed to generate contact sheet: {}", e);
        }
    }

    Ok(())
}

async fn process_vod(
    vod_url: &str,
    output_dir: &str,
    config: &Settings,
    state: &Arc<Mutex<RecordingState>>,
) -> Result<()> {
    let url = Url::parse(vod_url).context("Invalid VOD URL")?;
    let path_segments: Vec<&str> = url.path_segments().unwrap().collect();
    let video_id = path_segments.get(1).context("Invalid VOD URL format")?;
    let timestamp = url
        .query_pairs()
        .find(|(key, _)| key == "t")
        .map(|(_, value)| value.into_owned());

    let output_filename = format!("vod_{}.ts", video_id);
    let output_path = PathBuf::from(output_dir).join(&output_filename);
    {
        let mut state = state.lock().await;
        state.current_file = Some(output_path.clone());
    }

    println!("Downloading VOD: {}", vod_url);
    let mut streamlink_args = vec![
        "--twitch-disable-ads",
        vod_url,
        "best",
        "-o",
        output_path.to_str().unwrap(),
    ];

    let timestamp_string: Option<String> = timestamp.map(|ts| ts.to_string());
    if let Some(ts) = &timestamp_string {
        streamlink_args.push("--twitch-start-time");
        streamlink_args.push(ts);
    }

    let streamlink_status = Command::new("streamlink").args(&streamlink_args).status()?;

    if streamlink_status.success() {
        println!("VOD download complete. Processing file...");
        process_file(&config, &output_path).await?;
    } else {
        println!("Failed to download VOD.");
    }

    Ok(())
}

async fn process_clip(
    clip_url: &str,
    output_dir: &str,
    config: &Settings,
    state: &Arc<Mutex<RecordingState>>,
) -> Result<()> {
    let url = Url::parse(clip_url).context("Invalid Clip URL")?;
    let path_segments: Vec<&str> = url.path_segments().unwrap().collect();

    let clip_id = match url.host_str() {
        Some("clips.twitch.tv") => path_segments.last(),
        _ => path_segments.last(),
    }
    .context("Invalid Clip URL format")?;

    let clips_folder = PathBuf::from(output_dir).join("clips");
    std::fs::create_dir_all(&clips_folder)?;

    let output_filename = format!("{}.ts", clip_id);
    let output_path = clips_folder.join(&output_filename);

    {
        let mut state = state.lock().await;
        state.current_file = Some(output_path.clone());
    }

    println!("Downloading Clip: {}", clip_url);
    let streamlink_args = vec![
        "--twitch-disable-ads",
        clip_url,
        "best",
        "-o",
        output_path.to_str().unwrap(),
    ];

    let streamlink_status = Command::new("streamlink").args(&streamlink_args).status()?;

    if streamlink_status.success() {
        println!("Clip download complete. Processing file...");
        process_file(&config, &output_path).await?;
    } else {
        println!("Failed to download Clip.");
    }

    Ok(())
}

async fn cleanup(config: &Settings, state: &Arc<Mutex<RecordingState>>) -> Result<()> {
    let state = state.lock().await;
    if let Some(current_file) = &state.current_file {
        println!("Processing last recorded/downloaded file...");
        process_file(config, current_file).await?;
    }
    Ok(())
}

async fn generate_contact_sheet(mp4_filepath: &PathBuf) -> Result<PathBuf> {
    let mut args = args::application_args();
    args.filenames = vec![mp4_filepath.to_str().unwrap().to_string()];
    args.grid = vcsr::models::Grid { x: 4, y: 6 };
    args.num_samples = Some(24);
    args.output_path = Some(
        mp4_filepath
            .with_extension("jpg")
            .to_str()
            .unwrap()
            .to_string(),
    );
    args.show_timestamp = true;
    args.vcs_width = 1500;

    let dir_entry = WalkDir::new(mp4_filepath)
        .into_iter()
        .filter_map(|e| e.ok())
        .find(|e| e.path() == mp4_filepath)
        .context("Failed to create DirEntry")?;

    let contact_sheet =
        vcsr_process_file(&dir_entry, &mut args).context("Failed to generate contact sheet")?;

    println!(
        "[vcsr] Generated contact sheet: {}",
        contact_sheet.display()
    );

    Ok(contact_sheet)
}
