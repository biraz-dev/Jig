mod storage;
mod tui;

use chrono::{Datelike, Local, Utc};
use clap::{Parser, Subcommand};
use jig::{calendar, model::State, units};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    version,
    about = "Jig — a local artist's utility workbench",
    after_help = "No command opens the dashboard. All data stays in the selected JSON file."
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
    /// Evaluate arithmetic; percent is postfix division by 100; ans reuses the last result
    Calc { expression: String },
    /// List saved calculations (oldest first)
    History,
    /// Convert a length, e.g. units '1 1/2in' mm
    Units { input: String, target: String },
    /// A simple ordered checklist
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
            let n = state.calculate(&expression)?;
            println!("{}", state.number(n));
        }
        Command::History => {
            for h in &state.history {
                println!("{} = {}", h.expression, state.number(h.result));
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
        Command::Precision { decimals } => state.preferences.decimals = decimals as usize,
        Command::Tasks { action } => match action.unwrap_or(TaskAction::List) {
            TaskAction::List => {
                for t in &state.tasks {
                    println!(
                        "{} [{}] {}",
                        t.id,
                        if t.complete { "x" } else { " " },
                        t.text
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
                            let marker = if state
                                .deadlines
                                .iter()
                                .any(|d| d.at.with_timezone(&Local).date_naive() == *day)
                            {
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
