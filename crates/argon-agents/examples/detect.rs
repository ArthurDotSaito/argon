use std::time::Instant;

use argon_agents::{AgentSpec, Detection, Support, detect_all};
use argon_core::Config;

#[tokio::main]
async fn main() {
    let started = Instant::now();
    let results = detect_all(&Config::default()).await;
    let elapsed = started.elapsed();
    for (spec, support, detection) in &results {
        print_row(spec, support, detection);
    }
    println!("detected {} agents in {elapsed:.2?}", results.len());
}

fn print_row(spec: &AgentSpec, support: &Support, detection: &Detection) {
    println!("{}", spec.label);
    println!("  support:  {}", describe_support(support));
    println!("  status:   {}", describe_detection(detection));
    println!("  version:  {}", version_of(detection));
    println!("  model:    {}", describe_model(spec));
    println!("  command:  {}", describe_command(spec));
}

fn describe_support(support: &Support) -> String {
    match support {
        Support::Supported => "supported".to_string(),
        Support::NotYet { reason } => format!("not yet ({reason})"),
    }
}

fn describe_detection(detection: &Detection) -> String {
    match detection {
        Detection::Found { .. } => "found".to_string(),
        Detection::NotFound => "not found in PATH".to_string(),
        Detection::NeedsLogin { hint } => format!("needs login ({hint})"),
        Detection::Error(message) => format!("error ({message})"),
    }
}

fn version_of(detection: &Detection) -> &str {
    match detection {
        Detection::Found { version } => version,
        _ => "-",
    }
}

fn describe_model(spec: &AgentSpec) -> String {
    if spec.default_model.is_empty() {
        "(chosen by the agent session)".to_string()
    } else {
        spec.default_model.clone()
    }
}

fn describe_command(spec: &AgentSpec) -> String {
    let mut parts = vec![spec.command.display().to_string()];
    parts.extend(spec.args.iter().cloned());
    parts.join(" ")
}
