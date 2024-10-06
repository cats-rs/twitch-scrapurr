use anyhow::{Context, Result};
use chrono::Local;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use vcsr::{args, process_file as vcsr_process_file};
use walkdir::WalkDir;

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
    let config = load_config()?;
    let twitch_username = std::env::args().nth(1).unwrap_or_else(|| {
        println!("Streamer Username to record:");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).unwrap();
        input.trim().to_string()
    });

    let state = Arc::new(Mutex::new(RecordingState { current_file: None }));
    let state_clone = Arc::clone(&state);

    tokio::select! {
        result = record_stream(&twitch_username, &config, state) => {
            if let Err(e) = result {
                eprintln!("Recording error: {}", e);
            }
        }
        _ = tokio::signal::ctrl_c() => {
            println!("Received Ctrl+C, shutting down gracefully ᗜˬᗜ ");
        }
    }

    cleanup(&config, state_clone).await?;

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

async fn record_stream(
    username: &str,
    config: &Settings,
    state: Arc<Mutex<RecordingState>>,
) -> Result<()> {
    let user_vod_folder = PathBuf::from(&config.output_folder)
        .join(username)
        .join("vods");
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
    let mp4_filepath = if config.convert_to_mp4 {
        let mp4_filepath = ts_filepath.with_extension("mp4");

        if config.use_ffmpeg_convert {
            let ffmpeg_status = Command::new("ffmpeg")
                .args(&[
                    "-i",
                    ts_filepath.to_str().unwrap(),
                    "-c",
                    "copy",
                    mp4_filepath.to_str().unwrap(),
                ])
                .status()?;

            if ffmpeg_status.success() {
                std::fs::remove_file(ts_filepath)?;
                println!("Converted and saved as: {:?}", mp4_filepath);
            }
        } else {
            std::fs::rename(ts_filepath, &mp4_filepath)?;
            println!("Renamed and saved as: {:?}", mp4_filepath);
        }
        mp4_filepath
    } else {
        println!("Saved as: {:?}", ts_filepath);
        ts_filepath.clone()
    };

    if config.generate_contact_sheet {
        generate_contact_sheet(&mp4_filepath).await?;
    }

    Ok(())
}

async fn cleanup(config: &Settings, state: Arc<Mutex<RecordingState>>) -> Result<()> {
    let state = state.lock().await;
    if let Some(current_file) = &state.current_file {
        println!("Processing last recorded file...");
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
