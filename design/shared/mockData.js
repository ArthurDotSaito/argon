// Mock data compartilhado para os mockups do Argon (GPUI / GPUI Kit Desktop Client)
window.MOCK_DATA = {
  currentUser: {
    login: "arthur-dev",
    name: "Arthur Santos",
    avatar: "https://images.unsplash.com/photo-1534528741775-53994a69daeb?w=100&h=100&fit=crop&crop=faces",
    orgs: ["acme-corp", "argondv", "opensource-labs"]
  },

  categories: [
    { id: "inbox", label: "Inbox", icon: "inbox", count: 18, desc: "Todos os PRs em ordem cronológica" },
    { id: "needs_you", label: "Needs You", icon: "user-check", count: 7, desc: "Onde você foi marcado como reviewer ou assignee" },
    { id: "your_prs", label: "Your PRs", icon: "git-pull-request", count: 5, desc: "Pull Requests criados por você" },
    { id: "waiting_others", label: "Waiting Others", icon: "clock", count: 3, desc: "Seus PRs aguardando review de terceiros" },
    { id: "changes_requested", label: "Changes Requested", icon: "alert-circle", count: 2, desc: "Seus PRs com alterações solicitadas" },
    { id: "ready_to_merge", label: "Ready to Merge", icon: "check-circle-2", count: 4, desc: "Seus PRs aprovados prontos para merge" },
    { id: "already_done", label: "Already Done", icon: "archive", count: 9, desc: "PRs de outros que você revisou e estão prontos" }
  ],

  groups: [
    { id: "frontend", name: "Frontend Core", color: "#a855f7" },
    { id: "backend", name: "Backend Services", color: "#f59e0b" },
    { id: "infra", name: "Infra & DevOps", color: "#06b6d4" }
  ],

  repositories: [
    { id: "argon-desktop", name: "argondv/argon-desktop", openPrs: 4 },
    { id: "acme-api", name: "acme-corp/api-gateway", openPrs: 6 },
    { id: "acme-web", name: "acme-corp/web-app", openPrs: 5 },
    { id: "billing-svc", name: "acme-corp/billing-service", openPrs: 3 }
  ],

  aiRunners: [
    {
      id: "claude-code",
      name: "Claude Code",
      badge: "Anthropic CLI",
      models: ["Claude 3.7 Sonnet (Thinking)", "Claude 3.5 Sonnet", "Claude 3.5 Haiku", "Claude Opus 4.5"],
      defaultPrompt: "Você é um revisor sênior de código Rust/GPUI. Analise este PR focando em:\n1. Segurança de memória e concorrência\n2. Performance e alocações desnecessárias\n3. Conformidade com arquitetura do projeto\n4. Sugestões de melhorias diretas em código."
    },
    {
      id: "antigravity",
      name: "Antigravity Agent",
      badge: "DeepMind Multi-Agent",
      models: ["Antigravity Pro (Architecture + QA)", "Antigravity Flash", "Antigravity Lite"],
      defaultPrompt: "Execute uma análise estrita de arquitetura Clean Arch & SOLID. Identifique regressões de contrato de API, testes faltantes e valide a tipagem estrita."
    },
    {
      id: "opencode",
      name: "OpenCode",
      badge: "Local / Ollama",
      models: ["DeepSeek-R1 70B", "Qwen 2.5 Coder 32B", "Llama 3.3 70B"],
      defaultPrompt: "Analise o diff linha por linha gerando comentários concisos e práticos em português."
    },
    {
      id: "codex",
      name: "Codex",
      badge: "OpenAI Engine",
      models: ["o3-mini (High reasoning)", "o1", "GPT-4o"],
      defaultPrompt: "Verifique potenciais bugs lógicos, corner-cases em testes unitários e consistência da documentação."
    },
    {
      id: "commandcode",
      name: "CommandCode",
      badge: "Terminal Native",
      models: ["CLI Agent v2", "CLI Fast"],
      defaultPrompt: "Gere um resumo executivo dos commits, valide se quebra compatibilidade reversa e elabore o checklist de aprovação."
    }
  ],

  agentSessions: [
    {
      id: "session-101",
      prId: "PR-101",
      repo: "acme-corp/web-app",
      runner: "Claude Code",
      model: "Claude 3.7 Sonnet (Thinking)",
      status: "completed",
      statusLabel: "Revisão Concluída",
      updatedAt: "Há 12 min",
      summary: "3 vulnerabilidades potenciais encontradas e 2 refatorações de performance sugeridas."
    },
    {
      id: "session-105",
      prId: "PR-105",
      repo: "acme-corp/web-app",
      runner: "Antigravity Agent",
      model: "Antigravity Pro",
      status: "running",
      statusLabel: "Executando testes...",
      updatedAt: "Há 2 min",
      summary: "Verificando side-effects no pipeline de exportação assíncrona."
    },
    {
      id: "session-109",
      prId: "PR-109",
      repo: "acme-corp/api-gateway",
      runner: "OpenCode",
      model: "DeepSeek-R1 70B",
      status: "waiting_input",
      statusLabel: "Aguardando Confirmação",
      updatedAt: "Há 45 min",
      summary: "Deseja que eu gere os testes de regressão para a camada de validação?"
    }
  ],

  pullRequests: [
    {
      id: "PR-101",
      number: 101,
      title: "feat: redesign dashboard layout with GPUI Kit native widgets",
      repo: "acme-corp/web-app",
      author: {
        name: "Beatriz Lima",
        username: "bialima",
        avatar: "https://images.unsplash.com/photo-1494790108377-be9c29b29330?w=100&h=100&fit=crop&crop=faces"
      },
      branch: { base: "main", head: "feature/dashboard-redesign" },
      category: "needs_you",
      isYourPr: false,
      isReviewer: true,
      createdAt: "Há 6 dias",
      updatedAt: "Há 20 min",
      reviewStatus: "review_requested",
      commentsCount: 12,
      commitsCount: 6,
      filesChangedCount: 8,
      additions: 342,
      deletions: 98,
      ciStatus: { state: "success", passed: 14, total: 14 },
      description: "Este PR reformula todo o layout do dashboard principal usando os novos componentes nativos do GPUI Kit.\n\nPrincipais alterações:\n- Substituição das listas antigas pelo virtualizer nativo de alto desempenho\n- Suporte a temas dinâmicos (Dark / Zinc / OLED)\n- Integração com streaming de métricas via WebSocket",
      commits: [
        { hash: "7f8b91a", message: "feat: scaffold new dashboard container with GPUI view", author: "Beatriz Lima", time: "Há 6 dias" },
        { hash: "3c2d4e5", message: "refactor: extract widget grid into responsive layout", author: "Beatriz Lima", time: "Há 5 dias" },
        { hash: "9a0b1c2", message: "perf: integrate virtual scrolling for 10k+ rows", author: "Beatriz Lima", time: "Há 3 dias" },
        { hash: "5e6f7a8", message: "fix: prevent memory leak on live telemetry unmount", author: "Beatriz Lima", time: "Há 2 dias" },
        { hash: "1d2e3f4", message: "test: add integration test suite for dashboard state", author: "Beatriz Lima", time: "Há 1 dia" },
        { hash: "8c9d0e1", message: "style: adjust padding and borders according to GPUI design tokens", author: "Beatriz Lima", time: "Há 20 min" }
      ],
      actions: [
        { name: "Rust Cargo Check & Clippy", status: "success", duration: "1m 12s", workflow: "ci.yml" },
        { name: "GPUI Render Test Suite (macOS & Windows)", status: "success", duration: "3m 40s", workflow: "ci.yml" },
        { name: "Security Audit & Cargo Deny", status: "success", duration: "48s", workflow: "security.yml" },
        { name: "Automated Linter / rustfmt", status: "success", duration: "25s", workflow: "lint.yml" }
      ],
      diffs: [
        {
          file: "src/ui/dashboard/container.rs",
          oldPath: "src/ui/dashboard/container.rs",
          newPath: "src/ui/dashboard/container.rs",
          status: "modified",
          hunks: [
            {
              header: "@@ -45,10 +45,18 @@ impl DashboardView {",
              lines: [
                { type: "context", oldNum: 45, newNum: 45, content: "    pub fn render(&mut self, cx: &mut ViewContext<Self>) -> impl IntoElement {" },
                { type: "context", oldNum: 46, newNum: 46, content: "        div()" },
                { type: "delete", oldNum: 47, newNum: null, content: "-           .flex()" },
                { type: "delete", oldNum: 48, newNum: null, content: "-           .flex_col()" },
                { type: "delete", oldNum: 49, newNum: null, content: "-           .child(LegacyDashboardHeader::new())" },
                { type: "add", oldNum: null, newNum: 47, content: "+           .flex()" },
                { type: "add", oldNum: null, newNum: 48, content: "+           .flex_col()" },
                { type: "add", oldNum: null, newNum: 49, content: "+           .bg(cx.theme().background)" },
                { type: "add", oldNum: null, newNum: 50, content: "+           .child(GpuiKitHeader::new().with_breadcrumbs(true))" },
                { type: "add", oldNum: null, newNum: 51, content: "+           .child(" },
                { type: "add", oldNum: null, newNum: 52, content: "+               VirtualList::new(self.metrics.clone())" },
                { type: "add", oldNum: null, newNum: 53, content: "+                   .overscan(10)" },
                { type: "add", oldNum: null, newNum: 54, content: "+                   .item_height(px(48.0))" },
                { type: "add", oldNum: null, newNum: 55, content: "+           )" },
                { type: "context", oldNum: 50, newNum: 56, content: "    }" }
              ]
            }
          ]
        }
      ]
    },
    {
      id: "PR-105",
      number: 105,
      title: "feat: async export with progress indicator and cancel token",
      repo: "acme-corp/web-app",
      author: {
        name: "Arthur Santos",
        username: "arthur-dev",
        avatar: "https://images.unsplash.com/photo-1534528741775-53994a69daeb?w=100&h=100&fit=crop&crop=faces"
      },
      branch: { base: "main", head: "feature/async-export" },
      category: "waiting_others",
      isYourPr: true,
      isReviewer: false,
      createdAt: "Há 4 dias",
      updatedAt: "Há 2 horas",
      reviewStatus: "pending_review",
      commentsCount: 5,
      commitsCount: 3,
      filesChangedCount: 4,
      additions: 185,
      deletions: 22,
      ciStatus: { state: "success", passed: 14, total: 14 },
      description: "Implementa worker em background assíncrono para exportações volumosas de relatórios em CSV e Parquet sem travar a UI em GPUI.",
      commits: [
        { hash: "e1a2b3c", message: "feat: add tokio background task handle with mpsc", author: "Arthur Santos", time: "Há 4 dias" },
        { hash: "d4e5f6a", message: "feat: wire cancel token to UI progress bar button", author: "Arthur Santos", time: "Há 3 dias" }
      ],
      actions: [
        { name: "Rust Cargo Check & Clippy", status: "success", duration: "1m 05s", workflow: "ci.yml" },
        { name: "Export Integration Tests", status: "success", duration: "2m 10s", workflow: "ci.yml" }
      ],
      diffs: []
    },
    {
      id: "PR-109",
      number: 109,
      title: "refactor: extract shared form validation into reusable traits",
      repo: "acme-corp/api-gateway",
      author: {
        name: "Carlos Mendes",
        username: "cmendes",
        avatar: "https://images.unsplash.com/photo-1507003211169-0a1dd7228f2d?w=100&h=100&fit=crop&crop=faces"
      },
      branch: { base: "main", head: "refactor/form-validation" },
      category: "needs_you",
      isYourPr: false,
      isReviewer: true,
      createdAt: "Há 4 dias",
      updatedAt: "Há 5 horas",
      reviewStatus: "review_requested",
      commentsCount: 3,
      commitsCount: 2,
      filesChangedCount: 6,
      additions: 120,
      deletions: 240,
      ciStatus: { state: "success", passed: 10, total: 10 },
      description: "Remove duplicações nas checagens de payload HTTP usando Rust Traits genéricas e validação compile-time.",
      commits: [],
      actions: [],
      diffs: []
    },
    {
      id: "PR-104",
      number: 104,
      title: "feat: redis connection pool with exponential backoff & healthcheck",
      repo: "acme-corp/api-gateway",
      author: {
        name: "Fernanda Lima",
        username: "ferlima",
        avatar: "https://images.unsplash.com/photo-1517841905240-472988babdf9?w=100&h=100&fit=crop&crop=faces"
      },
      branch: { base: "main", head: "feat/redis-pool" },
      category: "needs_you",
      isYourPr: false,
      isReviewer: true,
      createdAt: "Há 3 dias",
      updatedAt: "Há 1 dia",
      reviewStatus: "review_requested",
      commentsCount: 4,
      commitsCount: 4,
      filesChangedCount: 5,
      additions: 210,
      deletions: 45,
      ciStatus: { state: "success", passed: 12, total: 12 },
      description: "Substitui conexão única de Redis por pool multiplexado com failover automático.",
      commits: [],
      actions: [],
      diffs: []
    },
    {
      id: "PR-102",
      number: 102,
      title: "feat: rate limiting middleware with token bucket algorithm",
      repo: "acme-corp/api-gateway",
      author: {
        name: "Arthur Santos",
        username: "arthur-dev",
        avatar: "https://images.unsplash.com/photo-1534528741775-53994a69daeb?w=100&h=100&fit=crop&crop=faces"
      },
      branch: { base: "main", head: "feat/rate-limiter" },
      category: "ready_to_merge",
      isYourPr: true,
      isReviewer: false,
      createdAt: "Há 2 dias",
      updatedAt: "Há 3 horas",
      reviewStatus: "approved",
      commentsCount: 7,
      commitsCount: 3,
      filesChangedCount: 3,
      additions: 145,
      deletions: 12,
      ciStatus: { state: "success", passed: 10, total: 10 },
      description: "Middleware de rate limiting por IP e token de cliente com suporte a headers padrão IETF RFC 6585.",
      commits: [],
      actions: [],
      diffs: []
    },
    {
      id: "PR-250",
      number: 250,
      title: "feat: stripe webhook signature verification & idempotency key cache",
      repo: "acme-corp/billing-service",
      author: {
        name: "Arthur Santos",
        username: "arthur-dev",
        avatar: "https://images.unsplash.com/photo-1534528741775-53994a69daeb?w=100&h=100&fit=crop&crop=faces"
      },
      branch: { base: "main", head: "feat/stripe-webhooks" },
      category: "changes_requested",
      isYourPr: true,
      isReviewer: false,
      createdAt: "Há 2 dias",
      updatedAt: "Há 1 hora",
      reviewStatus: "changes_requested",
      commentsCount: 9,
      commitsCount: 5,
      filesChangedCount: 7,
      additions: 390,
      deletions: 70,
      ciStatus: { state: "failed", passed: 8, total: 9 },
      description: "Validação HMAC SHA-256 para eventos do Stripe com mitigação contra replay attack.",
      commits: [],
      actions: [],
      diffs: []
    },
    {
      id: "PR-106",
      number: 106,
      title: "fix: memory leak in image renderer under high concurrency stress test",
      repo: "acme-corp/web-app",
      author: {
        name: "Mariana Souza",
        username: "msouza",
        avatar: "https://images.unsplash.com/photo-1544005313-94ddf0286df2?w=100&h=100&fit=crop&crop=faces"
      },
      branch: { base: "main", head: "fix/image-renderer-leak" },
      category: "already_done",
      isYourPr: false,
      isReviewer: true,
      createdAt: "Há 3 dias",
      updatedAt: "Há 4 horas",
      reviewStatus: "approved",
      commentsCount: 6,
      commitsCount: 2,
      filesChangedCount: 3,
      additions: 64,
      deletions: 32,
      ciStatus: { state: "success", passed: 14, total: 14 },
      description: "Corrigido descarte inadequado de buffers de texturas OpenGL/Metal na renderização do GPUI.",
      commits: [],
      actions: [],
      diffs: []
    }
  ]
};
