const MIN_FENCE_LEN: usize = 3;

struct OpenFence {
    len: usize,
    is_target: bool,
    lines: Vec<String>,
}

fn backtick_run(line: &str) -> usize {
    line.chars().take_while(|&c| c == '`').count()
}

pub(crate) fn last_fenced_block(message: &str, info: &str) -> Option<String> {
    let mut open: Option<OpenFence> = None;
    let mut last: Option<String> = None;
    for line in message.lines() {
        let trimmed = line.trim();
        let run = backtick_run(trimmed);
        match open.take() {
            None if run >= MIN_FENCE_LEN && !trimmed[run..].contains('`') => {
                open = Some(OpenFence {
                    len: run,
                    is_target: trimmed[run..].trim() == info,
                    lines: Vec::new(),
                });
            }
            None => {}
            Some(fence) if run >= fence.len && run == trimmed.len() => {
                if fence.is_target {
                    last = Some(fence.lines.join("\n"));
                }
            }
            Some(mut fence) => {
                fence.lines.push(line.to_owned());
                open = Some(fence);
            }
        }
    }
    if let Some(fence) = open.filter(|fence| fence.is_target) {
        last = Some(fence.lines.join("\n"));
    }
    last
}
