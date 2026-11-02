use argon_core::fixtures;
use argon_core::{
    ChecksState, DEFAULT_HOST, Involvement, LatestReview, Mergeable, Paths, PrId, PrState,
    PullRequest, ReviewDecision, ReviewState,
};
use argon_store::Store;
use chrono::{DateTime, Duration, Utc};

const ME: &str = "seed-me";
const PR_COUNT: u64 = 1_000;
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

Writes 1,000 fake pull requests into <scratch dir>/argon.db.
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
    store.replace_prs(DEFAULT_HOST, &prs)?;
    store.save_viewer(DEFAULT_HOST, ME, &[])?;
    println!(
        "seeded {} pull requests into {}",
        prs.len(),
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
        },
        ..fixtures::pr()
    }
}
