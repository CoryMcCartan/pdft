mod app;
mod cli;
mod model;
mod ops;
mod render;
mod tui;

use anyhow::Result;
use clap::Parser;

use cli::{Cli, Command, collect_page_indices};

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::Info { file }) => {
            let doc = model::document::PdfDocument::open(&file)?;
            println!("File: {}", file.display());
            println!("Pages: {}", doc.page_count());

            // Page sizes — group identical sizes
            let dims = doc.page_dimensions();
            let mut size_groups: Vec<((f64, f64), Vec<usize>)> = Vec::new();
            for (i, &dim) in dims.iter().enumerate() {
                // Round to 0.1pt for grouping
                let key = ((dim.0 * 10.0).round() / 10.0, (dim.1 * 10.0).round() / 10.0);
                if let Some(group) = size_groups.iter_mut().find(|(k, _)| *k == key) {
                    group.1.push(i + 1);
                } else {
                    size_groups.push((key, vec![i + 1]));
                }
            }
            for ((w, h), pages) in &size_groups {
                let w_in = w / 72.0;
                let h_in = h / 72.0;
                let size_name = match (w_in.round() as u32, h_in.round() as u32) {
                    (9, 11) | (8, 11) => " (Letter)",
                    (8, 14) => " (Legal)",
                    (6, 9) => " (A5)",
                    (8, 12) | (9, 12) => " (A4)",
                    (12, 17) | (11, 17) => " (Tabloid)",
                    _ => "",
                };
                if size_groups.len() == 1 {
                    println!("Size: {w_in:.1}\" × {h_in:.1}\"{size_name}");
                } else {
                    let page_desc = if pages.len() <= 3 {
                        pages.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(", ")
                    } else {
                        format!("{}-{}", pages[0], pages[pages.len() - 1])
                    };
                    println!("Size: {w_in:.1}\" × {h_in:.1}\"{size_name} (pages {page_desc})");
                }
            }

            // PDF metadata from document info dictionary
            doc.print_metadata();

            Ok(())
        }

        Some(Command::Merge { files, output }) => {
            let paths: Vec<&std::path::Path> = files.iter().map(|p| p.as_path()).collect();
            ops::merge::merge_files(&paths, &output)
        }

        Some(Command::Split {
            file,
            pages,
            output,
        }) => {
            let ranges: Vec<_> = pages.into_iter().flatten().collect();
            let indices = collect_page_indices(&ranges);
            ops::split::split_pages(&file, &indices, &output)
        }

        Some(Command::Delete {
            file,
            pages,
            output,
        }) => {
            let ranges: Vec<_> = pages.into_iter().flatten().collect();
            let indices = collect_page_indices(&ranges);
            let out = output.unwrap_or_else(|| file.clone());
            ops::delete::delete_pages(&file, &indices, &out)
        }

        Some(Command::Text {
            file,
            page,
            width,
            height,
        }) => {
            let (term_w, term_h) = crossterm::terminal::size().unwrap_or((80, 60));
            let width = if width == 0 { term_w } else { width };
            let height = if height == 0 { term_h } else { height };
            let data = std::fs::read(&file)?;
            let pdf = hayro::hayro_syntax::Pdf::new(data)
                .map_err(|e| anyhow::anyhow!("failed to parse PDF: {e:?}"))?;
            let lines = render::text_layout::extract_text_grid(&pdf, page - 1, width, height)?;
            for line in &lines {
                println!("{line}");
            }
            Ok(())
        }

        Some(Command::View { file, halfblock, text, page, watch }) => {
            tui::event_loop::run(&file, halfblock, text, page, watch)
        }

        None => {
            if let Some(file) = cli.file {
                tui::event_loop::run(&file, false, false, None, false)
            } else {
                use clap::CommandFactory;
                Cli::command().print_help()?;
                println!();
                Ok(())
            }
        }
    }
}
