//! Downloading videos from sites (YouTube, TikTok, VK, Rutube and the
//! many others yt-dlp supports) into a channel's media storage.
//!
//! * [`tool`] — finding, checking and installing yt-dlp
//! * [`info`] — what a link offers: title, qualities (pure, tested)
//! * [`job`] — reading a link and the download job

pub mod info;
pub mod job;
pub mod tool;
