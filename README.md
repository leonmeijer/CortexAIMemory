# Cortex AI Memory

**Een gedeeld kennisgeheugen voor AI-agents.**

Cortex geeft AI-agents een gemeenschappelijk brein. In plaats van dat elke agent opnieuw begint, delen ze codestructuur, plannen, beslissingen en voortgang via een centrale kennisgraaf — aangedreven door IndentiaGraph DB.

---

## Wat is Cortex?

Cortex is een Rust-gebaseerde service die AI-coding-agents coördineert op complexe projecten. Het biedt:

- **Kennisgraaf** — Codestructuur en relaties opgeslagen in SurrealDB (IndentiaGraph), toegankelijk voor alle agents
- **Semantisch zoeken** — Vind code op betekenis via IndentiaGraph ingebouwde vectorzoekopdracht
- **Plan- en taakbeheer** — Gestructureerde workflows met afhankelijkheden, stappen en voortgangsbewaking
- **Meertalige parsing** — Tree-sitter ondersteuning voor Rust, TypeScript, Python, Go en 13 andere talen
- **Multi-project workspaces** — Groepeer gerelateerde projecten met gedeelde context, contracten en mijlpalen
- **MCP-integratie** — 20 mega-tools beschikbaar voor Claude Code, OpenAI Agents en Cursor
- **Auto-sync** — Bestandswatcher houdt de kennisbank up-to-date terwijl je codeert
- **Authenticatie** — Google OAuth2, OIDC en wachtwoord-login met deny-by-default beveiliging
- **Chat WebSocket** — Realtime conversationele AI via Claude-integratie
- **Event-systeem** — Live CRUD-notificaties via WebSocket
- **NATS-integratie** — Inter-process eventsync voor multi-instance deployments
- **Neurale skills** — Emergente kennisclusters met activatie, triggers en levenscyclusbeheer
- **Spreading activation** — Multi-hop neurale ophaling via SYNAPSE-verbindingen

---

## Hoe werkt het?

```
┌─────────────────────────────────────────────────────────────┐
│                     JOUW AI-AGENTS                          │
│           (Claude Code / OpenAI / Cursor)                   │
└──────────┬──────────────────┬───────────────────┬───────────┘
           │ MCP Protocol     │ WebSocket         │ REST API
           ▼                  ▼                   ▼
┌─────────────────────────────────────────────────────────────┐
│                        CORTEX                               │
│                   (20 MCP Mega-Tools)                       │
│                                                             │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌────────────┐  │
│  │   Auth   │  │   Chat   │  │  Events  │  │   Config   │  │
│  │OIDC+Pass │  │  Claude  │  │ Live WS  │  │   YAML +   │  │
│  │  + JWT   │  │  Streams │  │ + NATS   │  │  env vars  │  │
│  └──────────┘  └──────────┘  └──────────┘  └────────────┘  │
└─────────────────────────────┬───────────────────────────────┘
                              │
          ┌───────────────────┼────────────────────┐
          ▼                   ▼                    ▼
┌──────────────────┐  ┌──────────┐     ┌──────────────────┐
│    SURREALDB     │  │  NATS    │     │   TREE-SITTER    │
│  (IndentiaGraph) │  │          │     │                  │
│                  │  │• Events  │     │• 17 talen        │
│• Kennisgraaf     │  │• Chat    │     │• AST-parsing     │
│• Plannen & taken │  │  relay   │     │• Symbolen        │
│• Vector search   │  │          │     │• Oproepgraaf     │
│• Notes & skills  │  └──────────┘     └──────────────────┘
└──────────────────┘
```

### Gegevensstroom

1. **Sync** — Cortex parseert je codebase met Tree-sitter en slaat symbolen, oproepen en imports op in IndentiaGraph
2. **Query** — AI-agents vragen via MCP-tools naar relevante code, plannen en beslissingen
3. **Schrijven** — Agents leggen beslissingen, voortgang en notities vast — beschikbaar voor alle andere agents
4. **Activatie** — Het neurale skill-systeem detecteert kennisclusters en activeert contextinjectie via hooks

---

## Snel aan de slag

### 1. Start de backends

```bash
docker compose up -d
```

Dit start: **SurrealDB** (kennisgraaf) en **NATS** (eventsync).

Daarna start je Cortex:

```bash
cortex serve
```

### 2. Configureer je AI-tool

Voeg toe aan je MCP-configuratie (`~/.claude/mcp.json`):

```json
{
  "mcpServers": {
    "cortex": {
      "command": "cortex-mcp",
      "env": {
        "SURREALDB_URL": "ws://localhost:8000",
        "SURREALDB_USERNAME": "root",
        "SURREALDB_PASSWORD": "root",
        "SURREALDB_NAMESPACE": "cortex",
        "SURREALDB_DATABASE": "memory",
        "NATS_URL": "nats://localhost:4222"
      }
    }
  }
}
```

Of automatisch instellen:

```bash
cortex setup-claude
```

### 3. Registreer je project

```bash
# Via CLI
cortex-cli project create --name "mijn-project" --path /pad/naar/code

# Of via de AI-agent (MCP)
# "Registreer dit project en sync de codebase"
```

---

## Wat kunnen je agents doen?

### Code verkennen
```
"Zoek alle functies die authenticatie afhandelen"
"Wat importeert dit bestand?"
"Wat is de impact van het wijzigen van UserService?"
"Toon de oproepgraaf van parse_file()"
```

### Werk beheren
```
"Maak een plan voor OAuth-ondersteuning"
"Wat is de volgende taak die ik moet oppakken?"
"Leg vast dat we JWT boven sessies hebben gekozen"
"Toon de kritieke pad van dit plan"
```

### Gesynchroniseerd blijven
```
"Welke beslissingen zijn genomen over caching?"
"Toon de projectroadmap"
"Welke taken blokkeren de release?"
"Hoe staan de mijlpalen ervoor?"
```

### Meerdere projecten coördineren
```
"Maak een workspace voor onze microservices"
"Voeg het API-contract toe dat alle services delen"
"Wat is de voortgang van de cross-project mijlpalen?"
```

---

## Installatie

### Vereisten

| Vereiste | Versie | Doel |
|----------|--------|------|
| Rust | 1.75+ | Bouwen vanuit broncode |
| Docker | 20.10+ | SurrealDB en NATS draaien |
| Docker Compose | 2.0+ | Services orkestreren |

### Vanuit broncode bouwen

```bash
git clone <repository-url>
cd CortexAIMemory

# Bouw alle binaries
cargo build --release

# Binaries in target/release/:
#   cortex       — hoofdserver
#   cortex-cli   — CLI-tool
#   cortex-mcp   — MCP-server voor AI-tools
#   cortex-mem   — geheugenworker-daemon
```

### Docker

```bash
# Start alle services
docker compose up -d
```

### Vanuit binaire release

```bash
# Zet de binaries in je PATH
cp target/release/cortex /usr/local/bin/
cp target/release/cortex-cli /usr/local/bin/
cp target/release/cortex-mcp /usr/local/bin/
```

---

## Configuratie

Het systeem gebruikt een gelaagde configuratie: **omgevingsvariabelen > config.yaml > standaarden**.

```bash
cp config.yaml.example config.yaml
# Pas config.yaml aan
```

### config.yaml voorbeeld

```yaml
server:
  port: 8080

graph_backend: "indentiagraph"

surrealdb:
  url: "ws://localhost:8000"
  namespace: "cortex"
  database: "memory"
  username: "root"
  password: "root"

nats:
  url: "nats://localhost:4222"

chat:
  default_model: "claude-opus-4-6"
```

### Omgevingsvariabelen

| Variabele | Standaard | Beschrijving |
|-----------|-----------|--------------|
| `SURREALDB_URL` | `ws://localhost:8000` | SurrealDB WebSocket-URL |
| `SURREALDB_USERNAME` | `root` | SurrealDB gebruikersnaam |
| `SURREALDB_PASSWORD` | `root` | SurrealDB wachtwoord |
| `SURREALDB_NAMESPACE` | `cortex` | SurrealDB namespace |
| `SURREALDB_DATABASE` | `memory` | SurrealDB database |
| `GRAPH_BACKEND` | `indentiagraph` | Graafbackend (`indentiagraph` of `neo4j`) |
| `NATS_URL` | *(optioneel)* | NATS-server URL |
| `CHAT_DEFAULT_MODEL` | `claude-opus-4-6` | Standaard Claude-model voor chat |
| `RUST_LOG` | `info` | Logniveau |

---

## Ondersteunde talen

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

## Integraties

| Platform | Status | Documentatie |
|----------|--------|--------------|
| **Claude Code** | Volledig ondersteund | [Installatiegids](docs/integrations/claude-code.md) |
| **OpenAI Agents** | Volledig ondersteund | [Installatiegids](docs/integrations/openai.md) |
| **Cursor** | Volledig ondersteund | [Installatiegids](docs/integrations/cursor.md) |

---

## Documentatie

| Gids | Beschrijving |
|------|--------------|
| [Installatie](docs/setup/installation.md) | Volledige installatie-instructies |
| [Aan de slag](docs/guides/getting-started.md) | Stap-voor-stap tutorial |
| [API-referentie](docs/api/reference.md) | Volledige REST API-documentatie |
| [MCP-tools](docs/api/mcp-tools.md) | Alle 20 MCP mega-tools met voorbeelden |
| [Workspaces](docs/guides/workspaces.md) | Multi-projectcoördinatie |
| [Multi-agent workflows](docs/guides/multi-agent-workflow.md) | Meerdere agents coördineren |
| [Authenticatie](docs/guides/authentication.md) | JWT + OAuth/OIDC instellen |
| [Chat & WebSocket](docs/guides/chat-websocket.md) | Realtime chat en events |
| [Kennisnotities](docs/guides/knowledge-notes.md) | Contextuele kennisopname |

