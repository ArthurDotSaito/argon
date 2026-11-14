use argon_core::fixtures;
use argon_core::{
    ChecksState, DEFAULT_HOST, Involvement, Issue, IssueLabel, IssueState, IssueStateReason,
    LatestReview, LinkedPr, Mergeable, Paths, PrId, PrState, PullRequest, ReviewDecision,
    ReviewState, SubIssues,
};
use argon_store::Store;
use chrono::{DateTime, Duration, Utc};

const ME: &str = "seed-me";
const PR_COUNT: u64 = 1_000;
const ISSUE_COUNT: u64 = 1_000;
const FIRST_ISSUE_NUMBER: u64 = 5_000;
const SPREAD_MINUTES: i64 = 60 * 24 * 60;
const OWNERS: [&str; 4] = ["orbita-labs", "nimbus-dev", "vela-oss", "atlas-cloud"];
const REPOS: [&str; 6] = [
    "scheduling-api",
    "patient-portal",
    "billing",
    "infra",
    "web-app",
    "ml-triage",
];
const LOGINS: [&str; 9] = [
    "marina-costa",
    "diego.r",
    "lia_m",
    "bruno-s",
    "camila",
    "joao-pedro",
    "renata",
    "tiago-k",
    ME,
];
const KINDS: [&str; 6] = ["feat", "fix", "chore", "refactor", "perf", "docs"];
const SUBJECTS: [&str; 10] = [
    "reagendamento automático quando médico cancela",
    "timezone errado no lembrete de consulta",
    "exportação assíncrona de faturas",
    "índice composto em appointments",
    "tela de consentimento LGPD",
    "vazamento de memória no batch loader",
    "webhooks de status da consulta",
    "modelo de triagem com calibração",
    "suporte a config em TOML",
    "guia de plugins",
];
const ISSUE_TYPES: [&str; 3] = ["Bug", "Feature", "Task"];
const LABELS: [(&str, &str); 7] = [
    ("bug", "d73a4a"),
    ("enhancement", "a2eeef"),
    ("good first issue", "7057ff"),
    ("lgpd", "0e8a16"),
    ("performance", "fbca04"),
    ("needs-triage", "ededed"),
    (
        "precisa de decisão de produto antes de entrar na sprint",
        "c5def5",
    ),
];
const MILESTONES: [&str; 3] = ["Q4 estabilidade", "Agenda 2.0", "Faturamento v3"];

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len() as u64) as usize]
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
}

const DATA_DIR_ENV: &str = "ARGON_DATA_DIR";
const USAGE: &str =
    "usage: ARGON_DATA_DIR=<scratch dir> cargo run -p argon-app --example seed_store

Writes 1,000 fake pull requests and 1,000 fake issues into <scratch dir>/argon.db.
Run the app with the same ARGON_DATA_DIR and with GH_CONFIG_DIR pointing to an
empty directory; otherwise a logged-in gh account syncs and replaces the seed.";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args()
        .skip(1)
        .any(|arg| arg == "--help" || arg == "-h")
    {
        println!("{USAGE}");
        return Ok(());
    }
    if std::env::var_os(DATA_DIR_ENV).is_none_or(|dir| dir.is_empty()) {
        eprintln!(
            "{DATA_DIR_ENV} is not set; refusing to seed the real data directory

{USAGE}"
        );
        std::process::exit(2);
    }
    let paths = Paths::resolve();
    std::fs::create_dir_all(&paths.data_dir)?;
    let mut store = Store::open(&paths.db_file())?;
    let now = Utc::now();
    let mut rng = Rng(0x5eed);
    let prs: Vec<PullRequest> = (0..PR_COUNT)
        .map(|index| fake_pr(index, now, &mut rng))
        .collect();
    let issues: Vec<Issue> = (0..ISSUE_COUNT)
        .map(|index| fake_issue(index, now, &mut rng))
        .collect();
    store.upsert_prs(DEFAULT_HOST, &prs)?;
    store.upsert_issues(DEFAULT_HOST, &issues)?;
    store.save_viewer(DEFAULT_HOST, ME, &[])?;
    println!(
        "seeded {} pull requests and {} issues into {}",
        prs.len(),
        issues.len(),
        paths.db_file().display()
    );
    println!(
        "start the app with {DATA_DIR_ENV}={} and GH_CONFIG_DIR set to an empty directory, otherwise the next sync replaces the seed",
        paths.data_dir.display()
    );
    Ok(())
}

fn fake_pr(index: u64, now: DateTime<Utc>, rng: &mut Rng) -> PullRequest {
    let owner = rng.pick(&OWNERS);
    let repo = rng.pick(&REPOS);
    let author = rng.pick(&LOGINS);
    let kind = rng.pick(&KINDS);
    let subject = rng.pick(&SUBJECTS);
    let number = 100 + index;
    let minutes = i64::try_from(rng.below(SPREAD_MINUTES as u64)).unwrap_or_default();
    let updated_at = now - Duration::minutes(minutes);
    let created_at = updated_at - Duration::days(i64::try_from(rng.below(20)).unwrap_or(0));
    let state = match rng.below(10) {
        0 => PrState::Merged,
        1 => PrState::Closed,
        _ => PrState::Open,
    };
    let closed_at = (state != PrState::Open).then_some(updated_at);
    let reviewers: Vec<String> = (0..rng.below(4))
        .map(|_| rng.pick(&LOGINS).to_string())
        .filter(|login| login != author)
        .collect();
    let latest_reviews = reviewers
        .iter()
        .take(usize::try_from(rng.below(3)).unwrap_or(0))
        .map(|login| LatestReview {
            author: login.clone(),
            state: if rng.chance(70) {
                ReviewState::Approved
            } else {
                ReviewState::ChangesRequested
            },
            submitted_at: updated_at - Duration::minutes(5),
        })
        .collect();
    PullRequest {
        id: PrId(format!("SEED_{index:04}")),
        host: DEFAULT_HOST.into(),
        repo: format!("{owner}/{repo}"),
        number,
        title: format!("{kind}: {subject} ({number})"),
        url: format!("https://{DEFAULT_HOST}/{owner}/{repo}/pull/{number}"),
        author: author.into(),
        is_draft: rng.chance(8),
        state,
        head: format!(
            "{kind}/{}-{number}",
            subject.split(' ').next().unwrap_or(kind)
        ),
        additions: u32::try_from(rng.below(1_500)).unwrap_or(0),
        deletions: u32::try_from(rng.below(600)).unwrap_or(0),
        changed_files: u32::try_from(1 + rng.below(40)).unwrap_or(1),
        comments: u32::try_from(rng.below(12)).unwrap_or(0),
        review_decision: match rng.below(4) {
            0 => Some(ReviewDecision::Approved),
            1 => Some(ReviewDecision::ChangesRequested),
            2 => Some(ReviewDecision::ReviewRequired),
            _ => None,
        },
        checks: match rng.below(5) {
            0 => Some(ChecksState::Failure),
            1 => Some(ChecksState::Pending),
            2 => None,
            _ => Some(ChecksState::Success),
        },
        mergeable: if rng.chance(10) {
            Mergeable::Conflicting
        } else {
            Mergeable::Mergeable
        },
        requested_reviewers: reviewers,
        latest_reviews,
        head_committed_at: updated_at - Duration::minutes(30),
        created_at,
        updated_at,
        closed_at,
        involvement: Involvement {
            requested: rng.chance(15),
            assigned: rng.chance(5),
            authored: author == ME,
            reviewed: rng.chance(10),
            ..Involvement::default()
        },
        ..fixtures::pr()
    }
}

fn fake_issue(index: u64, now: DateTime<Utc>, rng: &mut Rng) -> Issue {
    let owner = rng.pick(&OWNERS);
    let repo = rng.pick(&REPOS);
    let author = rng.pick(&LOGINS);
    let subject = rng.pick(&SUBJECTS);
    let number = FIRST_ISSUE_NUMBER + index;
    let minutes = i64::try_from(rng.below(SPREAD_MINUTES as u64)).unwrap_or_default();
    let updated_at = now - Duration::minutes(minutes);
    let created_at = updated_at - Duration::days(i64::try_from(rng.below(60)).unwrap_or(0));
    let (state, state_reason) = match rng.below(10) {
        0 => (IssueState::Closed, Some(IssueStateReason::Completed)),
        1 => (IssueState::Closed, Some(IssueStateReason::NotPlanned)),
        2 if rng.chance(30) => (IssueState::Closed, Some(IssueStateReason::Duplicate)),
        3 if rng.chance(30) => (IssueState::Open, Some(IssueStateReason::Reopened)),
        _ => (IssueState::Open, None),
    };
    let closed_at = (state == IssueState::Closed).then_some(updated_at);
    let mut assignees: Vec<String> = Vec::new();
    for _ in 0..rng.below(4) {
        let login = rng.pick(&LOGINS).to_string();
        if !assignees.contains(&login) {
            assignees.push(login);
        }
    }
    let mut labels: Vec<IssueLabel> = Vec::new();
    for _ in 0..rng.below(5) {
        let (name, color) = LABELS[usize::try_from(rng.below(LABELS.len() as u64)).unwrap_or(0)];
        if !labels.iter().any(|label| label.name == name) {
            labels.push(IssueLabel {
                name: name.into(),
                color: color.into(),
            });
        }
    }
    let linked_prs = if rng.chance(20) {
        vec![LinkedPr {
            repo: format!("{owner}/{repo}"),
            number: 100 + rng.below(PR_COUNT),
            state: match rng.below(4) {
                0 => PrState::Merged,
                1 => PrState::Closed,
                _ => PrState::Open,
            },
            is_draft: rng.chance(20),
        }]
    } else {
        Vec::new()
    };
    let sub_issues = rng.chance(15).then(|| {
        let total = u32::try_from(1 + rng.below(8)).unwrap_or(1);
        SubIssues {
            total,
            completed: u32::try_from(rng.below(u64::from(total) + 1)).unwrap_or(0),
        }
    });
    let last_mention_at = rng
        .chance(10)
        .then(|| updated_at - Duration::minutes(i64::try_from(rng.below(90)).unwrap_or(0)));
    Issue {
        id: PrId(format!("SEED_ISSUE_{index:04}")),
        host: DEFAULT_HOST.into(),
        repo: format!("{owner}/{repo}"),
        number,
        title: format!("{subject} ({number})"),
        url: format!("https://{DEFAULT_HOST}/{owner}/{repo}/issues/{number}"),
        author: author.into(),
        state,
        state_reason,
        issue_type: match rng.below(4) {
            3 => None,
            ix => Some(ISSUE_TYPES[usize::try_from(ix).unwrap_or(0)].into()),
        },
        involvement: Involvement {
            assigned: assignees.iter().any(|login| login == ME),
            authored: author == ME,
            mentioned: last_mention_at.is_some(),
            commented: rng.chance(10),
            ..Involvement::default()
        },
        labels,
        assignees,
        milestone: rng.chance(30).then(|| rng.pick(&MILESTONES).to_string()),
        comments: u32::try_from(rng.below(20)).unwrap_or(0),
        linked_prs,
        sub_issues,
        created_at,
        updated_at,
        closed_at,
        last_mention_at,
    }
}
