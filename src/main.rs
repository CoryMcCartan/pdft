mod app;
mod cli;
mod ext;
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
            let dims = doc.page_dimensions();
            println!("File: {}", file.display());
            println!("Pages: {}", doc.page_count());
            for (i, (w, h)) in dims.iter().enumerate() {
                println!("  Page {}: {:.0} × {:.0} pt", i + 1, w, h);
            }
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
            let data = std::fs::read(&file)?;
            let pdf = hayro::hayro_syntax::Pdf::new(data)
                .map_err(|e| anyhow::anyhow!("failed to parse PDF: {e:?}"))?;
            let lines = render::text_layout::extract_text_grid(&pdf, page - 1, width, height)?;
            for line in &lines {
                println!("{line}");
            }
            Ok(())
        }

        Some(Command::View { file }) => {
            tui::event_loop::run(&file)
        }

        None => {
            if let Some(file) = cli.file {
                tui::event_loop::run(&file)
            } else {
                use clap::CommandFactory;
                Cli::command().print_help()?;
                println!();
                Ok(())
            }
        }
    }
}
