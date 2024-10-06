# Twitch Scrapurr

A versatile Rust tool to record Twitch streams, download VODs and clips using streamlink. This tool allows you to capture Twitch content in the best available quality without the need for API tokens or complex setups.

## TODO:

- [ ] Switch to [rust-ffmpeg](https://github.com/zmwangx/rust-ffmpeg) or just ffmpeg for contact sheet generation
- [x] CLI arg for passing vod links to save
- [x] CLI arg for save location 

## Features

- Record Twitch streams in real-time
- Download Twitch VODs and clips
- Save streams as .ts files (more reliable in case of failures)
- Option to convert .ts files to .mp4
- Contact Sheet Generation
- Easy setup and usage
- Custom output directory support


## Requirements

- [ffmpeg](https://ffmpeg.org/)
- [streamlink](https://github.com/streamlink/streamlink)
- [cargo/rust](https://rustup.rs)

## Setup 

1. Download the requirements and ensure they're all in your PATH

2. Clone the repository

```bash
git clone https://github.com/cats-rs/twitch-scrapurr && cd twitch-scrapurr
```

3. Build the program:

```bash
cargo build --release
```

The built binary will be in `./target/release/twitch-scrapurr(.exe)`, you can then move this to a directory in which it will be added to PATH for easier use. 

## Usage

Run the program:

```bash
twitch-scrapurr -u {USERNAME}          # Record a live stream
twitch-scrapurr -v {VOD_OR_CLIP_URL}   # Download a VOD or clip
twitch-scrapurr -o {OUTPUT_DIRECTORY}  # Specify a custom output directory
```
### Command-line Options:

* -u, --username {USERNAME}: Specify the Twitch username to record
* -v, --video-url {VIDEO_URL}: Provide a Twitch VOD or clip URL to download
* -o, --output-dir {OUTPUT_DIR}: Set a custom output directory for recordings


If you run the program without arguments, you'll be prompted to enter a username.

On first run, you'll be asked to set an output folder for recordings. This will be saved in a `config.toml` for future use.

The program will continuously check for the stream and start recording when it's live. To stop, use Ctrl+C (Cmd+C on macOS). If interrupted during a stream, and convert_to_mp4 and generate_contact_sheet are enabled it will run those processes before stopping.

Note: If you choose not to convert the .ts files, you can watch them with media players like [MPV](https://mpv.io/) or [VLC](https://www.videolan.org/).

### Run with mprocs

An example config for [mprocs](https://github.com/pvolok/mprocs) is provided to allow running multiple instances for multiple streamers in an easy to manage way. Combine with a tool like [zellij](https://github.com/zellij-org/zellij) or [tmux](https://github.com/tmux/tmux/wiki) to allow for background checking and recording.

Once the mprocs.yaml file is set, simply run `mprocs`.

## Configuration

Edit `config.toml` to change settings:
- `convert_to_mp4`: Set to "True" or "False"
- `use_ffmpeg_convert`: Set to "True" or "False" (only applies if `convert_to_mp4` is True)
- `generate_contact_sheet` : "True" or "False"
- `check_interval`: Time in seconds between checks for live streams

## Disclaimer
Please note that recording and distributing Twitch streams without the permission of the content creator may violate Twitch's terms of service and could lead to legal consequences. Use this code responsibly and with respect for the creators whose content you are recording.
