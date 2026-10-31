mod storage;
mod tui;

use chrono::{Datelike, Local, NaiveDate, Utc};
use clap::{Parser, Subcommand};
use jig::{calendar, model::State, stl, timeline, units};
use std::{io::Read, path::PathBuf};

#[derive(Parser)]
#[command(
    version,
    about = "Jig — a local artist's utility workbench",
    after_help = "No command opens the dashboard. Workbench state stays in the selected JSON file; STL files are read locally."
)]
struct Cli {
    /// Portable state file (default: $XDG_DATA_HOME/jig/jig.json or ~/.local/share/jig/jig.json)
    #[arg(long, global = true)]
    state: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}
#[derive(Subcommand)]
enum Command {
    /// Evaluate arithmetic or a length conversion, e.g. calc '72 in mm'; ans reuses the last number
    Calc { expression: String },
    /// List saved calculations (oldest first)
    History,
    /// Convert a length, e.g. units '1 1/2in' mm
    Units { input: String, target: String },
    /// Inspect a local STL; source units must be chosen because STL does not store them
    Stl {
        file: PathBuf,
        #[arg(long, default_value = "mm", value_parser = ["mm", "cm", "in"])]
        units: String,
        /// Uniform size as a percentage of the original model
        #[arg(long, default_value_t = 100.0)]
        scale: f64,
        /// Additional X percentage multiplied by --scale (100 leaves X unchanged)
        #[arg(long, default_value_t = 100.0)]
        scale_x: f64,
        /// Additional Y percentage multiplied by --scale (100 leaves Y unchanged)
        #[arg(long, default_value_t = 100.0)]
        scale_y: f64,
        /// Additional Z percentage multiplied by --scale (100 leaves Z unchanged)
        #[arg(long, default_value_t = 100.0)]
        scale_z: f64,
        /// Estimated linear shrinkage percentage; not a prediction of warping
        #[arg(long)]
        shrinkage: Option<f64>,
        /// Enlarge to offset shrinkage instead of showing the smaller result
        #[arg(long, requires = "shrinkage")]
        compensate: bool,
    },
    /// A simple ordered checklist with optional due dates
    Tasks {
        #[command(subcommand)]
        action: Option<TaskAction>,
    },
    /// Deadlines use RFC3339 timestamps with an explicit timezone
    Deadline {
        #[command(subcommand)]
        action: DeadlineAction,
    },
    /// Month view and dated notes; markers: * deadline, + entry
    Calendar {
        #[arg(long, requires = "year", value_parser = clap::value_parser!(u32).range(1..=12))]
        month: Option<u32>,
        #[arg(long, requires = "month", value_parser = clap::value_parser!(i32).range(1..=9999))]
        year: Option<i32>,
        #[command(subcommand)]
        action: Option<EntryAction>,
    },
    /// Read-only task timeline around DATE (YYYY-MM-DD); defaults to this week
    Timeline {
        date: Option<NaiveDate>,
        /// Show the calendar month rather than Monday through Sunday
        #[arg(long)]
        month: bool,
    },
    /// Set displayed decimal places (internal precision is unchanged)
    Precision {
        #[arg(value_parser = clap::value_parser!(u8).range(0..=10))]
        decimals: u8,
    },
}
#[derive(Subcommand)]
enum TaskAction {
    List,
    /// Remove every completed task, preserving the unfinished checklist
    ClearCompleted,
    Add {
        text: String,
    },
    Edit {
        id: u64,
        text: String,
    },
    /// Toggle completion
    Done {
        id: u64,
    },
    /// Set a due date (RFC3339 with timezone), or omit AT to remove the date
    Due {
        id: u64,
        at: Option<String>,
    },
    /// Set a start (RFC3339 with timezone) on a dated task, or omit AT to clear it
    Start {
        id: u64,
        at: Option<String>,
    },
    /// Feature an unfinished dated task in the large countdown
    Star {
        id: u64,
    },
    Delete {
        id: u64,
    },
    /// Move to a 1-based position
    Move {
        id: u64,
        position: usize,
    },
}
#[derive(Subcommand)]
enum DeadlineAction {
    List,
    Add { title: String, at: String },
    Edit { id: u64, title: String, at: String },
    Delete { id: u64 },
    Feature { id: u64 },
    Countdown,
}
#[derive(Subcommand)]
enum EntryAction {
    List,
    Add {
        date: String,
        title: String,
    },
    Edit {
        id: u64,
        date: String,
        title: String,
    },
    Delete {
        id: u64,
    },
}
fn default_path() -> Result<PathBuf, String> {
    if let Some(base) = std::env::var_os("XDG_DATA_HOME").filter(|p| PathBuf::from(p).is_absolute())
    {
        return Ok(PathBuf::from(base).join("jig/jig.json"));
    }
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".local/share/jig/jig.json"))
        .ok_or("Set HOME or pass --state PATH".into())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("jig: {e}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let path = match cli.state {
        Some(path) => path,
        None => default_path()?,
    };
    let store = storage::Store::open(path)?;
    let mut state = store.load()?;
    if let Some(command) = cli.command {
        if execute(command, &mut state)? {
            store.save(&state)?;
        }
    } else {
        tui::run(&mut state, &store)?;
    }
    Ok(())
}
fn execute(command: Command, state: &mut State) -> Result<bool, String> {
    match command {
        Command::Calc { expression } => {
            state.calculate(&expression)?;
            let answer = state.history.last().ok_or("No calculation result")?;
            println!("{}", answer.formatted(state.preferences.decimals));
        }
        Command::History => {
            for h in &state.history {
                println!(
                    "{} = {}",
                    h.expression,
                    h.formatted(state.preferences.decimals)
                );
            }
            return Ok(false);
        }
        Command::Units { input, target } => {
            println!(
                "{} {}",
                state.number(units::convert(&input, &target)?),
                target
            );
            return Ok(false);
        }
        Command::Stl {
            file,
            units,
            scale,
            scale_x,
            scale_y,
            scale_z,
            shrinkage,
            compensate,
        } => {
            inspect_stl(
                &file,
                &units,
                scale,
                [scale_x, scale_y, scale_z],
                shrinkage,
                compensate,
                state,
            )?;
            return Ok(false);
        }
        Command::Precision { decimals } => state.preferences.decimals = decimals as usize,
        Command::Tasks { action } => match action.unwrap_or(TaskAction::List) {
            TaskAction::List => {
                for t in &state.tasks {
                    let due = if let Some(deadline) = state.task_deadline(t.id) {
                        format!(
                            "  due {}{}",
                            deadline.at.to_rfc3339(),
                            if state.featured == Some(deadline.id) {
                                " *"
                            } else {
                                ""
                            }
                        )
                    } else {
                        String::new()
                    };
                    let start = t
                        .start
                        .map(|at| format!("  start {}", at.to_rfc3339()))
                        .unwrap_or_default();
                    println!(
                        "{} [{}] {}{}{}",
                        t.id,
                        if t.complete { "x" } else { " " },
                        t.text,
                        due,
                        start
                    );
                }
                return Ok(false);
            }
            TaskAction::ClearCompleted => {
                println!("Removed {} completed tasks", state.remove_completed_tasks())
            }
            TaskAction::Add { text } => println!("{}", state.add_task(&text)?),
            TaskAction::Edit { id, text } => state.edit_task(id, &text)?,
            TaskAction::Done { id } => state.toggle_task(id)?,
            TaskAction::Due { id, at } => state.set_task_deadline(id, at.as_deref())?,
            TaskAction::Start { id, at } => state.set_task_start(id, at.as_deref())?,
            TaskAction::Star { id } => state.feature_task(id)?,
            TaskAction::Delete { id } => state.remove_task(id)?,
            TaskAction::Move { id, position } => state.move_task(id, position)?,
        },
        Command::Deadline { action } => match action {
            DeadlineAction::List => {
                for d in &state.deadlines {
                    println!(
                        "{} {} {} {}",
                        d.id,
                        if state.featured == Some(d.id) {
                            "*"
                        } else {
                            " "
                        },
                        d.at.to_rfc3339(),
                        d.title
                    );
                }
                return Ok(false);
            }
            DeadlineAction::Add { title, at } => {
                println!("{}", state.set_deadline(None, &title, &at)?)
            }
            DeadlineAction::Edit { id, title, at } => {
                state.set_deadline(Some(id), &title, &at)?;
            }
            DeadlineAction::Delete { id } => state.remove_deadline(id)?,
            DeadlineAction::Feature { id } => state.feature(id)?,
            DeadlineAction::Countdown => {
                let d = state
                    .deadlines
                    .iter()
                    .find(|d| Some(d.id) == state.featured)
                    .ok_or("No featured deadline")?;
                println!("{}: {}", d.title, jig::model::countdown(d.at, Utc::now()));
                return Ok(false);
            }
        },
        Command::Timeline { date, month } => {
            let scale = if month {
                timeline::Scale::Month
            } else {
                timeline::Scale::Week
            };
            let range = timeline::window(date.unwrap_or_else(|| Local::now().date_naive()), scale)?;
            println!("{} to {}", range.start, range.end);
            for row in timeline::rows(state, &Local) {
                let status = if row.complete { "Done" } else { "Open" };
                let dates = match (row.start, row.due) {
                    (Some(start), Some(due)) => format!("{start} to {due}"),
                    (_, Some(due)) => format!("due {due}"),
                    _ => "No date".to_owned(),
                };
                println!("{status}: {} · {dates}", row.title);
                let mut bar = vec![' '; range.days() as usize];
                if let Some(span) = timeline::clipped(&row, range) {
                    let first = span.offset as usize;
                    let last = first + span.days as usize - 1;
                    bar[first..=last].fill(if span.milestone { '*' } else { '=' });
                    if span.clipped_start {
                        bar[first] = '<';
                    }
                    if span.clipped_end {
                        bar[last] = '>';
                    }
                }
                println!("|{}|", bar.into_iter().collect::<String>());
            }
            return Ok(false);
        }
        Command::Calendar {
            year,
            month,
            action,
        } => match action {
            Some(EntryAction::List) => {
                for e in &state.entries {
                    println!("{} {} {}", e.id, e.date, e.title);
                }
                return Ok(false);
            }
            Some(EntryAction::Add { date, title }) => {
                println!("{}", state.set_entry(None, &title, &date)?)
            }
            Some(EntryAction::Edit { id, date, title }) => {
                state.set_entry(Some(id), &title, &date)?;
            }
            Some(EntryAction::Delete { id }) => state.remove_entry(id)?,
            None => {
                let today = Local::now().date_naive();
                let y = year.unwrap_or(today.year());
                let m = month.unwrap_or(today.month());
                println!("{y:04}-{m:02}\n Mo  Tu  We  Th  Fr  Sa  Su");
                for week in calendar::month(y, m)?.chunks(7) {
                    for day in week {
                        if let Some(day) = day {
                            let has_deadline = state.deadlines.iter().any(|d| {
                                state.is_deadline_active(d.id)
                                    && d.at.with_timezone(&Local).date_naive() == *day
                            });
                            let marker = if has_deadline {
                                '*'
                            } else if state.entries.iter().any(|e| e.date == *day) {
                                '+'
                            } else if *day == today {
                                '·'
                            } else {
                                ' '
                            };
                            print!("{:3}{marker}", day.day());
                        } else {
                            print!("    ");
                        }
                    }
                    println!();
                }
                return Ok(false);
            }
        },
    }
    Ok(true)
}

fn inspect_stl(
    path: &std::path::Path,
    units: &str,
    scale_percent: f64,
    axis_percent: [f64; 3],
    shrink_percent: Option<f64>,
    compensate: bool,
    state: &State,
) -> Result<(), String> {
    if !scale_percent.is_finite() || scale_percent <= 0.0 {
        return Err("Scale must be a finite percentage greater than zero".into());
    }
    if axis_percent
        .iter()
        .any(|percent| !percent.is_finite() || *percent <= 0.0)
    {
        return Err("Each axis scale must be a finite percentage greater than zero".into());
    }
    let unit = match units {
        "mm" => stl::LengthUnit::Millimeters,
        "cm" => stl::LengthUnit::Centimeters,
        "in" => stl::LengthUnit::Inches,
        _ => return Err("Use mm, cm or in for STL source units".into()),
    };
    let shrink = shrink_percent.map(stl::shrinkage).transpose()?;
    let factor = shrink.map_or(1.0, |shrink| {
        if compensate {
            shrink.compensation
        } else {
            shrink.remaining
        }
    });
    let file = std::fs::File::open(path)
        .map_err(|error| format!("Could not read STL {}: {error}", path.display()))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("Could not inspect STL {}: {error}", path.display()))?;
    if !metadata.is_file() {
        return Err("The STL path must refer to a file".into());
    }
    if metadata.len() > stl::MAX_FILE_BYTES as u64 {
        return Err("STL exceeds the 20 MiB file limit".into());
    }
    let mut bytes = Vec::new();
    file.take(stl::MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Could not read STL {}: {error}", path.display()))?;
    if bytes.len() > stl::MAX_FILE_BYTES {
        return Err("STL exceeds the 20 MiB file limit".into());
    }
    let mesh = stl::parse(&bytes)?;
    let scales = axis_percent.map(|percent| scale_percent / 100.0 * (percent / 100.0) * factor);
    let measurements = mesh.measure_axes(unit, scales)?;
    println!("STL: {} triangles", mesh.triangles.len());
    println!("Source units: {units} (STL carries no unit metadata)");
    println!("Scale: {}%", state.number(scale_percent));
    if axis_percent != [100.0; 3] {
        println!(
            "Axis scale: X {}%, Y {}%, Z {}% (multiplied by uniform scale)",
            state.number(axis_percent[0]),
            state.number(axis_percent[1]),
            state.number(axis_percent[2])
        );
    }
    if let Some(percent) = shrink_percent {
        println!(
            "Linear shrinkage estimate: {}% ({})",
            state.number(percent),
            if compensate {
                "compensated"
            } else {
                "forecast"
            }
        );
    }
    let [x, y, z] = measurements.dimensions_mm;
    println!(
        "Dimensions: {} x {} x {} mm",
        state.number(x),
        state.number(y),
        state.number(z)
    );
    println!(
        "Surface area: {} mm^2",
        state.number(measurements.surface_area_mm2)
    );
    if let Some(volume) = measurements.volume_mm3 {
        println!(
            "Volume: {} mm^3 ({} cm^3)",
            state.number(volume),
            state.number(volume / 1000.0)
        );
        println!("Volume assumes non-intersecting closed shells.");
    } else {
        println!("Volume: unavailable; mesh is not a verified closed, consistently oriented solid");
    }
    println!(
        "Mesh check: {} boundary edges, {} non-manifold edges, {} inconsistent edges, {} degenerate triangles",
        mesh.metrics.boundary_edges,
        mesh.metrics.non_manifold_edges,
        mesh.metrics.inconsistent_edges,
        mesh.metrics.degenerate_triangles
    );
    Ok(())
}
