use crate::config::HarnConfig;
use crate::{boxed_error, AppResult, ConfigCommand};

pub fn run(command: ConfigCommand) -> AppResult<()> {
    match command {
        ConfigCommand::Set { key, value } => set_value(&key, &value),
        ConfigCommand::Get { key } => get_value(&key),
        ConfigCommand::List => list_values(),
        ConfigCommand::Path => {
            println!("{}", HarnConfig::path().display());
            Ok(())
        }
    }
}

fn set_value(key: &str, value: &str) -> AppResult<()> {
    let mut config = HarnConfig::load();
    match key {
        "api_key" => config.api_key = value.to_string(),
        "openai_api_key" => config.openai_api_key = value.to_string(),
        "model" => config.model = value.to_string(),
        "openai_model" => config.openai_model = value.to_string(),
        "idle_timeout" => {
            config.idle_timeout = value
                .parse()
                .map_err(|_| boxed_error("idle_timeout must be an integer number of seconds"))?
        }
        "exclude_projects" => {
            config.exclude_projects = value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToOwned::to_owned)
                .collect();
        }
        _ => return Err(boxed_error(format!("unknown config key: {key}"))),
    }

    config.save()?;
    println!("Saved {key}.");
    Ok(())
}

fn get_value(key: &str) -> AppResult<()> {
    let config = HarnConfig::load();
    match key {
        "api_key" => println!("{}", config.api_key),
        "openai_api_key" => println!("{}", config.openai_api_key),
        "model" => println!("{}", config.model_name()),
        "openai_model" => println!("{}", config.openai_model_name()),
        "idle_timeout" => println!("{}", config.idle_timeout),
        "exclude_projects" => {
            if config.exclude_projects.is_empty() {
                println!("[]");
            } else {
                println!("{}", config.exclude_projects.join(","));
            }
        }
        _ => return Err(boxed_error(format!("unknown config key: {key}"))),
    }
    Ok(())
}

fn list_values() -> AppResult<()> {
    let config = HarnConfig::load();
    println!("api_key = {}", crate::display::mask_secret(&config.api_key));
    println!("openai_api_key = {}", crate::display::mask_secret(&config.openai_api_key));
    println!("model = {}", config.model_name());
    println!("openai_model = {}", config.openai_model_name());
    println!("idle_timeout = {}", config.idle_timeout);
    if config.exclude_projects.is_empty() {
        println!("exclude_projects = []");
    } else {
        println!(
            "exclude_projects = [{}]",
            config
                .exclude_projects
                .iter()
                .map(|item| format!("\"{item}\""))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    Ok(())
}
