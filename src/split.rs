use crossbeam_channel::Sender;
use std::path::Path;

pub enum StatusReport {
    Started,
    LinesParsed(usize),
    ChaptersSplit(usize),
    NewTitle(String),
    Error(anyhow::Error),
    Done,
}

pub async fn split_chapters(
    pattern: impl AsRef<str>,
    file: impl AsRef<Path>,
    folder: impl AsRef<Path>,
    start_chapter: usize,
    channel: Sender<StatusReport>,
) {
    channel.send(StatusReport::Started).unwrap();
    if let Err(e) =
        split_chapters_internal(pattern, file, folder, start_chapter, channel.clone()).await
    {
        channel.send(StatusReport::Error(e)).unwrap();
    } else {
        channel.send(StatusReport::Done).unwrap();
    }
}

async fn split_chapters_internal(
    pattern: impl AsRef<str>,
    file: impl AsRef<Path>,
    folder: impl AsRef<Path>,
    start_chapter: usize,
    channel: Sender<StatusReport>,
) -> anyhow::Result<()> {
    use regex::Regex;
    use tokio::fs::File;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let file = file.as_ref();
    let folder = folder.as_ref();
    let pattern = pattern.as_ref();
    let pattern = Regex::new(pattern)?;

    tokio::fs::create_dir_all(folder).await?;
    let file = File::open(file).await?;
    let reader = BufReader::new(file);

    let mut chapter_number = start_chapter;
    let mut line_number = 0usize;
    let mut chapter_text = String::new();

    let mut lines = reader.lines();
    while let Some(line) = lines.next_line().await? {
        if pattern.is_match(&line) {
            channel.send(StatusReport::NewTitle(line.clone())).unwrap();

            // Write the previous chapter text to file, if any
            if !chapter_text.is_empty() {
                let filename = folder.join(format!("{:04}.txt", chapter_number));
                let mut file = File::create(filename).await?;
                file.write_all(chapter_text.as_bytes()).await?;
                chapter_text.clear();
            }

            chapter_number += 1;
            channel
                .send(StatusReport::ChaptersSplit(chapter_number))
                .unwrap();
        }
        // Append the line to the current chapter text
        chapter_text.push_str(&line);
        chapter_text.push('\n');

        line_number += 1;
        if line_number % 1000 == 0 {
            channel
                .send(StatusReport::LinesParsed(line_number))
                .unwrap();
        }
    }

    // Write the last chapter text to file, if any
    if !chapter_text.is_empty() {
        let filename = folder.join(format!("{:04}.txt", chapter_number));
        let mut file = File::create(filename).await?;
        file.write_all(chapter_text.as_bytes()).await?;
    }

    Ok(())
}
