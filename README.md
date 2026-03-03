<p align="center">
  <img src="dist/logo-512.png" alt="CortexAIMemory" width="128" />
</p>

<h1 align="center">CortexAIMemory</h1>

<p align="center">
  <strong>AI-geheugen en orchestratielaag voor Indentia — koppelt Claude Code aan een kennisgraaf met episodisch geheugen.</strong>
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT%20AND%20BUSL--1.1-blue.svg" alt="Licentie"></a>
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/Rust-1.75+-orange.svg" alt="Rust"></a>
</p>

---

## Wat is CortexAIMemory?

CortexAIMemory is de AI-orchestratielaag van het Indentia-platform. Het geeft Claude Code en andere AI-agents een gedeeld, persistent geheugen: codestructuur, plannen, beslissingen, kennisnotities en episodisch geheugen worden centraal opgeslagen en doorzoekbaar gemaakt.

In plaats van elke sessie opnieuw te beginnen, bouwt CortexAIMemory een kennisgraaf op die groeit naarmate je codebase evolueert. Agents kunnen elkaars context lezen, taken oppakken, beslissingen terugvinden en patronen herkennen die eerder zijn vastgelegd.

Het systeem is opgezet rond twee kernprincipes: **structureel geheugen** (wat staat er in de code, wie hangt van wie af, welke beslissingen zijn genomen) en **episodisch geheugen** (wat is er wanneer gebeurd, wat wisten we op een bepaald moment in de tijd).

### Kernfunctionaliteit

- **Kennisgraaf (IndentiaGraph/SurrealDB)** — code-structuur, plannen, taken, beslissingen en notes als graaf
- **Native BM25 + vector search** — full-text en semantisch zoeken direct op de graph-backend
- **Episodisch geheugen (Graphiti-inspired)** — tijdgestempelde episoden, bi-temporele notes, historische queries
- **MCP-server** — 20 mega-tools voor Claude Code, OpenAI Agents en Cursor
- **cortex-mem** — memory worker daemon die Claude Code sessies automatisch vastlegt (poort 37777)
- **Tree-sitter parser** — 17 programmeertalen, inclusief HCL/Terraform
- **File watcher** — automatische code-synchronisatie bij elke opgeslagen wijziging
- **Authenticatie** — Google OAuth2, generieke OIDC, wachtwoord + JWT, deny-by-default middleware
- **Chat WebSocket** — real-time conversationele AI via Claude Code CLI (Nexus SDK)
- **Event systeem** — live CRUD-notificaties via WebSocket

---

## Architectuur

```
┌─────────────────────────────────────────────────────────────┐
│                    AI-AGENTS                                │
│           (Claude Code / OpenAI / Cursor)                   │
└──────────┬──────────────────┬───────────────────┬───────────┘
           │ MCP Protocol     │ WebSocket         │ REST API
           ▼                  ▼                   ▼
┌─────────────────────────────────────────────────────────────┐
│                   CORTEX (HTTP server)                      │
│              (20 MCP Mega-Tools, Axum 0.8)                  │
│                                                             │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌────────────┐  │
│  │   Auth   │  │   Chat   │  │  Events  │  │  Episodes  │  │
│  │OIDC+Pass │  │  Claude  │  │ Live WS  │  │ Bi-temporal│  │
│  │  + JWT   │  │  Streams │  │ + NATS   │  │   Memory   │  │
│  └──────────┘  └──────────┘  └──────────┘  └────────────┘  │
└─────────────────────────────┬───────────────────────────────┘
                              │
   ┌──────────────────────────┼──────────────────────────┐
   ▼                          ▼                          ▼
┌────────────────────┐  ┌──────────────────────┐  ┌──────────────┐
│ INDENTIAGRAPH API  │  │   SURREALDB ENGINE   │  │ TREE-SITTER  │
│                    │  │  (via indentiagraph) │  │              │
│ • Code graph       │  │ • BM25 zoeken        │  │ • 17 talen   │
│ • Plannen/taken    │  │ • Episoden           │  │ • AST parse  │
│ • Beslissingen     │  │ • Temporele queries  │  │ • Symbolen   │
└────────────────────┘  └──────────────────────┘  └──────────────┘
```

### Binaries

| Binary | Beschrijving |
|--------|-------------|
| `cortex` | Hoofdserver (was: `orchestrator`) |
| `cortex-cli` | CLI-tool (was: `orch`) |
| `cortex-mcp` | MCP-server voor AI-tools (was: `mcp_server`) |
| `cortex-mem` | Memory worker daemon (poort 37777) |
| `cortex-mem-hook` | Claude Code hook binary |

### Feature Flags

| Flag | Standaard | Beschrijving |
|------|-----------|-------------|
| `embedded-frontend` | nee | Embed React SPA in de binary |
| `vendored-openssl` | nee | Statische OpenSSL voor cross-compilatie |
| `nexus-memory` | nee | Activeert Nexus memory integratie voor chat |

---

## Installatie

### Vereisten

- Rust 1.75+
- Geen Docker nodig voor standaardgebruik (embedded SurrealKV)
- Optioneel: externe SurrealDB 2.x/3.x via `ws://...`

### Snel starten

```bash
# Clone de repository
git clone <repository-url>
cd CortexAIMemory

# Bouw en start de server
cargo build --release
./target/release/cortex serve --port 8080
```

### Configuratie (config.yaml)

Kopieer en pas `config.yaml` aan:

```yaml
server:
  port: 8080                        # SERVER_PORT

indentiagraph:
  # Persistent lokale opslag (non-ephemeral, zonder Docker)
  uri: "surrealkv://./.cortex/indentiagraph"  # SURREALDB_URL (of legacy INDENTIAGRAPH_URI)
  user: "root"                      # SURREALDB_USERNAME (alleen nodig voor remote ws/http)
  password: "root"                  # SURREALDB_PASSWORD (alleen nodig voor remote ws/http)
  namespace: "cortex"               # SURREALDB_NAMESPACE
  database: "memory"                # SURREALDB_DATABASE

chat:
  default_model: "claude-opus-4-6"  # CHAT_DEFAULT_MODEL
```

| Omgevingsvariabele | Beschrijving | Standaard |
|--------------------|-------------|-----------|
| `SURREALDB_URL` | SurrealDB verbindings-URI | `surrealkv://./.cortex/indentiagraph` |
| `SURREALDB_USERNAME` | SurrealDB gebruiker | `root` |
| `SURREALDB_PASSWORD` | SurrealDB wachtwoord | `root` |
| `SURREALDB_NAMESPACE` | SurrealDB namespace | `cortex` |
| `SURREALDB_DATABASE` | SurrealDB database | `memory` |
| `INDENTIAGRAPH_URI/USER/PASSWORD` | Legacy aliases (achterwaarts compatibel) | - |
| `CHAT_DEFAULT_MODEL` | Standaard Claude model | `claude-opus-4-6` |
| `RUST_LOG` | Log niveau | `info` |

---

## MCP-integratie (Claude Code)

### Bouwen

```bash
cargo build --release --bin cortex-mcp
```

### Configuratie in `~/.claude/mcp.json`

```json
{
  "mcpServers": {
    "cortex": {
      "command": "/pad/naar/cortex-mcp",
      "env": {
        "PO_SERVER_URL": "http://127.0.0.1:8080"
      }
    }
  }
}
```

### Overzicht van de 20 Mega-Tools

| Mega-Tool | Acties | Beschrijving |
|-----------|--------|-------------|
| `project` | 8 | Project CRUD, sync, roadmap |
| `plan` | 10 | Plan lifecycle, dependency graph, critical path |
| `task` | 13 | Taken, afhankelijkheden, blockers, context |
| `step` | 6 | Subtaken, voortgang bijhouden |
| `decision` | 12 | Beslissingen, tijdlijn, impact tracking |
| `constraint` | 5 | Plan constraints (performance, security, stijl) |
| `release` | 8 | Releasemanagement met taken en commits |
| `milestone` | 9 | Mijlpalen met voortgang en plan-koppeling |
| `commit` | 7 | Git commit tracking, bestandshistorie |
| `note` | 24 | Kennisnotities, semantisch zoeken, episodisch geheugen |
| `workspace` | 10 | Multi-project workspaces, topologie |
| `workspace_milestone` | 10 | Cross-project mijlpalen |
| `resource` | 6 | Gedeelde API-contracten, schema's |
| `component` | 8 | Servicetopologie en afhankelijkheden |
| `chat` | 7 | Chat sessies, berichten, delegatie |
| `feature_graph` | 6 | Feature grafen, automatisch bouwen vanuit code |
| `code` | 36 | Code zoeken, call grafen, impact analyse, communities, bridge |
| `admin` | 25 | Sync, watch, Knowledge Fabric, neural onderhoud, skills |
| `skill` | 12 | Neurale skills detectie, activatie, export/import |
| `analysis_profile` | 4 | Edge/fusion gewichten presets voor analyse |

---

## Episodisch Geheugen

CortexAIMemory ondersteunt Graphiti-geïnspireerd temporeel episodisch geheugen: ruwe tekst als tijdgestempelde episoden opslaan, bi-temporele notes en historische queries.

### Hoe het werkt

**Episoden** zijn ruwe teksteenheden met een tijdstempel (gesprekken, code-events, documenten, systeem-events). Ze worden opgeslagen met BM25-indexering voor snelle full-text zoekacties.

**Bi-temporele notes** hebben twee tijdstempels:
- `valid_at` — wanneer het feit waar werd
- `invalid_at` — wanneer het feit ophield waar te zijn (NULL = nu nog geldig)

Dit maakt het mogelijk te vragen: "Wat wisten we op 15 januari 2024?" zonder huidige kennis te verliezen.

### API-voorbeelden

```bash
# Episode opslaan (gesprek, code-event, document, systeem-event)
curl -X POST http://localhost:8080/api/episodes \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Sprint planning 2024-01-15",
    "content": "Team besloot auth te migreren van JWT naar PASETO...",
    "source": "conversation",
    "reference_time": "2024-01-15T09:00:00Z",
    "project_id": "uuid-van-project"
  }'

# Recente episoden ophalen
curl "http://localhost:8080/api/episodes?project_id=uuid&limit=20"

# Episoden doorzoeken (BM25)
curl "http://localhost:8080/api/episodes/search?query=auth+migratie&project_id=uuid"

# Wat wisten we op een bepaald moment? (temporele zoekopdracht)
curl "http://localhost:8080/api/notes/at-time?query=auth&at_time=2024-01-15T09:00:00Z&project_id=uuid"

# Note niet-destructief ongeldig markeren op een tijdstip
curl -X POST http://localhost:8080/api/notes/{id}/invalidate-temporal \
  -H "Content-Type: application/json" \
  -d '{"at_time": "2024-01-16T00:00:00Z"}'
```

### MCP-tools voor episodisch geheugen

De `note` mega-tool heeft 5 extra acties:

| Actie | Beschrijving |
|-------|-------------|
| `add_episode` | Ruwe tekst opslaan als tijdgestempelde episode |
| `get_episodes` | Recente episoden ophalen (filter op project_id, group_id) |
| `search_episodes` | BM25 zoeken over episode-inhoud |
| `search_at_time` | "Wat wisten we op datum X?" temporele notitiezoekopdracht |
| `invalidate_temporal` | Note ongeldig markeren op een specifiek tijdstip |

---

## cortex-mem (Claude Code Hook)

`cortex-mem` is een memory worker daemon die automatisch Claude Code sessies vastlegt als episoden in het geheugen.

### Hoe het werkt

1. `cortex-mem` draait als achtergrondproces op poort 37777
2. `cortex-mem-hook` is een lichtgewicht binary die als `PostToolUse` hook in Claude Code wordt geconfigureerd
3. Na elke tool-aanroep stuurt de hook de context naar de memory worker
4. De worker slaat dit op als episode in SurrealDB

### Installatie als Claude Code hook

```bash
# Bouw de hook binary
cargo build --release -p cortex-mem --bin cortex-mem-hook

# Bouw ook de worker binary
cargo build --release -p cortex-mem --bin cortex-mem

# Start de memory worker
./target/release/cortex-mem &

# Configureer in ~/.claude/settings.json
```

Voeg toe aan `~/.claude/settings.json`:

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "*",
        "hooks": [
          {
            "type": "command",
            "command": "/pad/naar/cortex-mem-hook"
          }
        ]
      }
    ]
  }
}
```

---

## API-overzicht

### Plannen en Taken

| Endpoint | Methode | Beschrijving |
|----------|---------|-------------|
| `/api/plans` | GET/POST | Plannen ophalen / aanmaken |
| `/api/plans/{id}/tasks` | POST | Taak toevoegen aan plan |
| `/api/tasks/{id}` | GET/PATCH | Taakdetails / bijwerken |
| `/api/tasks/{id}/dependencies` | POST | Afhankelijkheden toevoegen |
| `/api/tasks/{id}/blockers` | GET | Geblokkeerde taken ophalen |

### Code Exploratie

| Endpoint | Methode | Beschrijving |
|----------|---------|-------------|
| `/api/code/search` | GET | Semantisch zoeken in code |
| `/api/code/symbols/{path}` | GET | Symbolen in een bestand |
| `/api/code/callgraph` | GET | Functie-aanroepgraaf |
| `/api/code/impact` | GET | Impact-analyse bij wijzigingen |
| `/api/code/architecture` | GET | Codebase-overzicht |

### Kennisnotities

| Endpoint | Methode | Beschrijving |
|----------|---------|-------------|
| `/api/notes` | GET/POST | Notities ophalen / aanmaken |
| `/api/notes/search` | GET | Semantisch zoeken in notities |
| `/api/notes/at-time` | GET | Temporele notitiezoekopdracht |
| `/api/notes/{id}/invalidate-temporal` | POST | Notitie ongeldig markeren op tijdstip |

### Episodisch Geheugen

| Endpoint | Methode | Beschrijving |
|----------|---------|-------------|
| `/api/episodes` | GET/POST | Episoden ophalen / opslaan |
| `/api/episodes/search` | GET | BM25 zoeken in episoden |

### Workspaces

| Endpoint | Methode | Beschrijving |
|----------|---------|-------------|
| `/api/workspaces` | GET/POST | Workspaces beheren |
| `/api/workspaces/{slug}/projects` | GET/POST | Projecten in workspace |
| `/api/workspaces/{slug}/topology` | GET | Topologiegraaf |

### Authenticatie

| Endpoint | Methode | Beschrijving |
|----------|---------|-------------|
| `/auth/login` | POST | Inloggen (wachtwoord) |
| `/auth/google` | GET | Google OAuth URL ophalen |
| `/auth/me` | GET | Huidig gebruikersprofiel |

---

## Ondersteunde Programmeertalen

| Taal | Extensies |
|------|-----------|
| Rust | `.rs` |
| TypeScript | `.ts`, `.tsx` |
| JavaScript | `.js`, `.jsx` |
| Python | `.py` |
| Go | `.go` |
| Java | `.java` |
| C/C++ | `.c`, `.h`, `.cpp`, `.cc`, `.cxx`, `.hpp`, `.hxx` |
| Ruby | `.rb` |
| PHP | `.php` |
| Kotlin | `.kt`, `.kts` |
| Swift | `.swift` |
| Bash | `.sh`, `.bash` |
| C# | `.cs` |
| Scala | `.scala` |
| Zig | `.zig` |
| HCL/Terraform | `.tf`, `.tfvars` |

---

## Ontwikkeling

```bash
# Alle tests uitvoeren (mock backends, geen Docker vereist voor unit tests)
cargo test

# Linter
cargo clippy

# Formatter
cargo fmt

# Release build
cargo build --release
```

### Teststructuur

| Testbestand | Beschrijving |
|-------------|-------------|
| `tests/api_tests.rs` | HTTP API tests |
| `tests/integration_tests.rs` | Database integratietests |
| `tests/parser_tests.rs` | Parser tests |
| `tests/workspace_tests.rs` | Workspace tests |
| `crates/cortex-indentiagraph/` | SurrealDB implementatietests (221) |

### Nieuwe API-endpoint toevoegen

1. Handler toevoegen in `src/api/handlers.rs` of een nieuwe `*_handlers.rs`
2. Route registreren in `src/api/routes.rs`
3. Test schrijven in `tests/api_tests.rs`

---

## Licentie

MIT AND BUSL-1.1 — zie [LICENSE](LICENSE) voor details.

---

<p align="center">
  <i>Geef je AI-agents een gedeeld geheugen.</i>
</p>
