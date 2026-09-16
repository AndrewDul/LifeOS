# LifeOS — Research & Decisions

Living record of research performed, alternatives considered, sources, and
the decisions made from them. Product/architecture conclusions drawn from
this research are written into `LIFEOS_SPEC.md`; this document is the
evidence and reasoning behind those conclusions. Current engineering status
is in `PROJECT_STATE.md`.

Research performed: 2026-09-16. This space (LLM pricing/capabilities,
voice APIs, OSS project activity) moves fast — treat specific prices,
version numbers, and "current" claims below as snapshots to re-verify
before acting on them at implementation time, not permanent facts.

---

## 1. Cloud LLM providers

Research goal: pick a default provider/model for LifeOS's user-facing
conversation, while keeping the architecture provider-agnostic.

### 1a. Identity & economics

| Provider / Model | Tier | Released (recency) | Context window | Input $/1M | Output $/1M | Free tier |
|---|---|---|---|---|---|---|
| OpenAI GPT-5.6 Sol | Flagship reasoning | ~Aug 2026 (promo pricing to Nov 21, 2026) | 400K-class | $4.00 (promo, list $5.00) | $20.00 (promo, list $30.00) | None — new accounts prepay ≥$5; separate opt-in "free daily tokens" trades traffic for training data |
| OpenAI GPT-5.6 Terra | Mid tier | ~Jul 2026 | Same family | $2.00 | $12.00 | Same as above |
| OpenAI GPT-5.6 Luna | Budget/fast | ~Jul 2026 | Same family | $0.20 | $1.20 | Same as above |
| Anthropic Claude Opus 5 | Flagship reasoning | Jul 24, 2026 | 1M | $5.00 | $25.00 | No standing free tier; one-time $5 trial credit (phone verification) |
| **Anthropic Claude Sonnet 5** | Default workhorse | 2026 (current gen) | 1M | $2.00 | $10.00 | Same trial-credit model |
| Anthropic Claude Haiku 4.5 | Fast/cheap | 2025-gen, still current | 200K | $1.00 | $5.00 | Same trial-credit model |
| Anthropic Claude Fable 5.1 | Frontier ("most capable") | 2026 | 1M | $10.00 | $50.00 | Same trial credit; requires 30-day data retention (no ZDR) |
| Google Gemini 3.1 Pro (Preview) | Flagship reasoning | 2026, still "Preview" — GA repeatedly delayed | 200K std, higher band beyond | $2.00 (≤200K) / $4.00 (>200K) | $12.00 (≤200K) / $18.00 (>200K) | Not available on free tier (removed ~Apr 2026) |
| Google Gemini 3.8 Flash | Fast/cheap, native grounding | Sep 2, 2026 | Standard | $0.75 (promo to Dec 31 2026, then $1.50) | $3.75 (promo, then $7.50) | Yes — AI Studio free key (~15 RPM/1,500 RPD); free-tier prompts used to improve Google's models |
| xAI Grok 4.6 | Flagship | Aug 12, 2026 | 500K | $2.00 (<200K) / $4.00 (≥200K, whole request) | $6.00 (<200K) / $12.00 (≥200K) | No standing free tier; $25 promo signup credit |
| xAI Grok 4.3 | Cheaper general-purpose | 2026 | 1M | $1.25 | $2.50 | Same $25 signup credit |

Mistral was checked but not tabled — nothing in current search results
indicated it is genuinely competitive with the above four on reasoning
quality, tool-calling maturity, or Polish/English conversational quality
for this use case. Revisit if EU data-residency or open-weight
requirements become a priority.

### 1b. Capability & fit

| Provider / Model | Structured output / tool-calling | Reasoning reputation | PL/EN multilingual | Latency/speed | Native web/research | API privacy posture | OpenAI-shaped abstraction? |
|---|---|---|---|---|---|---|---|
| OpenAI GPT-5.6 | Very mature — native Structured Outputs with `strict:true`, reference-implementation function calling | Strong; top-tier general/coding benchmarks | Good general multilingual (MMMLU ~91%); no PL-specific data | Solid TTFT, not class-leading tok/s | Built-in web search tool | Not used for training by default (API); 30-day retention unless ZDR (expanded Aug 2026) | **Native** — this is the reference shape |
| **Anthropic Claude (Opus 5/Sonnet 5/Haiku 4.5)** | Very mature — strict tool schemas, structured `output_config.format`, Tool Runner helper, parallel tool use; best-in-class reputation for reliable multi-step agentic tool use | Leading on deep reasoning/long-horizon agentic benchmarks (Opus 5 topped a Sep 2026 leaderboard cited) | Strong general reputation; no PL-specific benchmark found — validate empirically | Sonnet/Haiku responsive; Opus trades speed for depth | Server-side web search + web fetch tools | **No training on API data by default; logs auto-delete after 7 days** (shortest of the group); 30-day retention optional via DPA; enterprise ZDR available (not Fable 5.1) | Own wire format (Messages API) — needs a real adapter |
| Google Gemini (3.1 Pro/3.8 Flash) | Function calling via standard `tools`/`tool_choice`; **mixed reliability** — multiple 2026 reports of tool-calling degradation in long conversations, malformed calls, schema mismatches | 3.1 Pro competitive on knowledge/multilingual (MMMLU 91.8%, Global PIQA 93.4% across 100 languages) — best raw multilingual numbers of the four | Best-evidenced multilingual breadth, but flagship "Pro" still only "Preview" — maturity risk | Flash genuinely fast (sub-600ms TTFT) — good for snappy voice/chat; Pro slower | **Best-in-class** — native Google Search grounding built in | **Free tier: prompts used to improve Google's products.** Paid tier (billed key/Vertex): not used for training. EEA/UK/CH: paid terms apply even to free tier | **Official OpenAI-compatible endpoint** (`/v1beta/openai/`) — trivial adapter |
| xAI Grok (4.6/4.3) | OpenAI-compatible surface; less battle-tested in production agentic contexts | Competitive frontier benchmarks; less independent long-horizon/agentic reputation | Least independent PL evidence of the four | Reasonable; large context | Native Web Search/X Search server tools (billed separately, $5/1K calls) | Standard commercial terms; less independent scrutiny found — treat cautiously pending direct verification | **Native OpenAI-compatible** (`api.x.ai/v1`) |

### Decision: default to Anthropic Claude Sonnet 5, behind a provider-agnostic abstraction

Reasoning:

1. **Tool-calling reliability is the deciding factor.** LifeOS's chat will
   constantly call internal tools (tasks, calendar, memory retrieval).
   Anthropic's tool-use surface has the strongest reputation of the four;
   Gemini has documented tool-calling reliability complaints in long
   conversations; Grok's tool-use ecosystem is comparatively less proven.
2. **Best privacy posture for a personal life-management app.** No
   training on API data by default, 7-day log retention (shortest of the
   group), ZDR available if needed. Google's default free-tier terms use
   prompts for training/human review — a trap to avoid for anything but
   the cheapest, non-sensitive Flash calls; OpenAI's default retention
   window (30 days) is longer absent a ZDR arrangement.
3. **Cost/quality balance fits an everyday conversational engine.** At
   $2/$10 per MTok, Sonnet 5 is mid-pack — cheaper than Opus 5, GPT-5.6
   Sol, Gemini 3.1 Pro, Grok 4.6 — while reasoning/agentic quality is
   reported near the top of the field.
4. **Multilingual quality is uncertain for everyone** — no provider has
   public Polish benchmarks. Treat as an open item (`LIFEOS_SPEC.md` §13),
   validate with a small internal Polish eval before treating Sonnet 5 as
   a permanent, unreviewed default.
5. **The same abstraction should allow routing within Anthropic's own
   tiers** (Haiku 4.5 for latency-critical trivial turns, Opus 5 for hard
   reasoning) without a vendor swap — just a model-ID config change.

Architecture requirement this implies: the provider abstraction must
normalize message/tool-call schemas across vendors, since Anthropic's
Messages API is not OpenAI-wire-compatible while Gemini's and Grok's are —
this asymmetry must be handled explicitly in the adapter layer, not
assumed away.

### Sources

- Anthropic API & data retention: https://platform.claude.com/docs/en/manage-claude/api-and-data-retention
- OpenAI GPT-5.6 pricing: https://www.layer3labs.io/guides/gpt-5-6-pricing , https://www.morphllm.com/openai-api-pricing , https://www.cloudzero.com/blog/openai-pricing/
- OpenAI Zero Data Retention (Aug 2026): https://openai.com/index/offering-zero-data-retention-for-frontier-models/
- OpenAI free-tier/credits status: https://meetily.ai/llm-privacy/openai , https://github.com/orgs/community/discussions/181068
- OpenAI Structured Outputs: https://platform.openai.com/docs/guides/structured-outputs
- Google Gemini pricing (primary): https://ai.google.dev/gemini-api/docs/pricing
- Google Gemini data-use terms: https://ai.google.dev/gemini-api/docs/zdr , https://meetily.ai/llm-privacy/gemini
- Google Gemini free-tier limits: https://tokenmix.ai/blog/gemini-api-free-tier-limits
- Google Gemini OpenAI-compatible endpoint: https://ai.google.dev/gemini-api/docs/openai
- Gemini tool-calling reliability reports: https://github.com/allenhutchison/obsidian-gemini/issues/328
- xAI Grok pricing/context: https://mem0.ai/blog/xai-grok-api-pricing , https://kingy.ai/blog/grok-4-6-price-benchmarks-api-cursor-context-window/
- xAI OpenAI-compatible API: https://docs.x.ai/overview
- Multilingual benchmarks: https://www.vellum.ai/blog/google-gemini-3-benchmarks
- Leaderboard/latency snapshots: https://www.swfte.com/ai/leaderboard , https://artificialanalysis.ai/models

### Uncertainty flags

1. Naming/version churn is severe (OpenAI cycled through several flagship
   generations within a year) — treat "GPT-5.6" as a snapshot.
2. Google's flagship "Pro" is unusually stalled in "Preview" — a GA
   flagship could reshuffle Google's position at any time.
3. Most pricing came from third-party aggregators, not vendor pages,
   except Google/Anthropic (fetched directly) — re-verify OpenAI/xAI
   figures against their own pricing pages before locking a budget.
4. No Polish-specific benchmark exists for any model — build a small
   internal Polish eval (10-20 prompts across LifeOS's actual domains)
   before finalizing.
5. Free-tier policies are the most volatile figures here — re-check at
   implementation time.
6. Grok has the thinnest independent documentation of the four.

---

## 2. Local helper LLM

Research goal: pick a shortlist of small, local (Ollama-hosted) models for
structured extraction, classification, PII detection/redaction, semantic
retrieval support, and context selection — **not** the user-facing chat
model.

### Hardware reality (measured — see `PROJECT_STATE.md` for full audit)

RTX 3050 Ti Laptop, **4GB VRAM** (confirmed via `nvidia-smi`), is the
binding constraint. In practice on Ollama/llama.cpp:

- Fully-in-VRAM budget ≈ 3.0–3.3GB usable after runtime/CUDA overhead
  minus KV cache. **Q4_K_M models up to ~4-5B params** fit cleanly with
  room for a few thousand tokens of context.
- A 7-8B Q4_K_M model (~4.7-5GB weights alone) does **not** fully fit and
  forces partial CPU offload — measured/credible data points found: Llama
  3.2 1B Q4_K_M ≈14 tok/s on this GPU class; Llama 3.1 8B Q4_K_M ≈4.3
  tok/s (partial offload). Roughly a 4-10x throughput penalty for
  partial offload vs full GPU residency.
- 64GB RAM / 12C-16T CPU is generous for a **CPU-only fallback** of 7-8B
  Q4 models, but still single-digit-to-low-teens tok/s, not GPU-class
  throughput. No CUDA toolkit is required — Ollama bundles its own CUDA
  runtime; the NVIDIA driver alone is sufficient.

### Comparison table

| Model | Params | Quant for ~4GB | License | Structured/JSON strength | Multilingual EN/PL | Context |
|---|---|---|---|---|---|---|
| Llama 3.2 1B/3B Instruct | 1B/3B | Q4_K_M (~1-2GB) | Meta Llama 3.2 Community (commercial OK <700M MAU) | Weak at ≤3B: one benchmark found only 47.8-56.5% JSON parse rate | EN good; PL untested, likely mediocre at this size | 128K |
| Llama 3.1 8B Instruct | 8B | Q4_K_M (~4.7-5GB, doesn't fully fit) | Meta Llama 3.1 Community | Rated "safest for strict JSON schema compliance" in a 2026 benchmark | EN strong; PL moderate (43.77% Open PL LLM Leaderboard) | 128K |
| Qwen2.5 7B Instruct | 7B | Q4_K_M (~4.4GB, doesn't fully fit) | Apache 2.0 | "Best quality-to-speed ratio" per benchmark; strong native JSON/tool-calling | EN strong; PL 53.35% (best non-Polish-native tested) | 128K |
| Qwen2.5 1.5B Instruct | 1.5B | Q4/Q8 (~1-1.6GB) | Apache 2.0 | 95.7% JSON parse rate at Q8 — competitive with 7B | EN good; PL untested at this size | 32K |
| **Qwen3-4B-Instruct-2507** | 4B | Q4_K_M (~2.5GB) | Apache 2.0 | Strong tool-calling/JSON; non-thinking variant avoids reasoning-token overhead | EN strong; PL uncertain — needs own testing | 262K |
| Qwen3.5-4B/-2B (new, Feb 2026) | 2B/4B/9B | Q4_K_M | Apache 2.0, no MAU restriction | Newest generation; **not yet verified in Ollama library, not benchmarked anywhere found** | Unverified | up to 1M (flagship) |
| Phi-4-mini | 3.8B | Q4_K_M (~2.3GB) | MIT | One benchmark found only 0.518 classification accuracy, 19.9pp below Phi-3.5-mini — inconsistent signal | EN good; PL: no data | 128K |
| Phi-3.5-mini | 3.8B | Q4_K_M (~2.3GB) | MIT | Poor per one benchmark: 5-50x repetition rate vs peers, 30.4% length compliance | EN good; PL: no data | 128K |
| Gemma 3 1B/4B | 1B/4B | Q4_K_M (~0.7-2.5GB) | Gemma Terms (commercial OK, no MAU cap) | Weak raw extraction (F1 0.039), recovers to F1 0.702 with a prompt-engineered wrapper — not usable "as-is" | Broadest claimed coverage (140 languages), no PL-specific score found | 32K |
| **Bielik-4.5B-v3.0-Instruct** (SpeakLeash) | ~5B | Q4_K_M = **2.88GB** | Apache 2.0 | Polish-native model; no direct JSON-compliance benchmark found — verify ourselves | **PL: purpose-built, best-in-class among small open models**; EN secondary | 32K |
| Bielik-11B-v2/v3-Instruct | 11B | Q4_K_M (~6-7GB — too big for 4GB VRAM) | Apache 2.0/CC-BY-NC depending on version — verify per release | Best PL benchmark scores among sub-15B models (57.52-58.14%) | PL excellent; EN good | 32K |
| Ministral/Mistral Small | 3B-22B+ | Varies | **Uncertain/inconsistent 2026 licensing** — verify directly on mistral.ai | Reported best-in-class agentic/JSON for the family generally | EN strong; PL: no data | Varies |
| PLLuM (Polish govt model family) | Multiple | GGUF via community quantizers | Stated freely available; exact terms need direct verification | Not benchmarked for JSON/structured extraction anywhere found | PL purpose-built (govt-funded); EN secondary | Unknown |

**Non-LLM complements identified as directly relevant:**

- **GLiNER** (`knowledgator/gliner-pii-small-v1.0`/`gliner-pii-base-v1.0`)
  — a lightweight zero-shot NER/span-tagging transformer (not generative),
  purpose-built for PII detection/redaction across 40+ entity types, runs
  on CPU. Likely a better fit than prompting a general LLM for the
  redaction job specifically — see `LIFEOS_SPEC.md` §4.3.
- **nomic-embed-text** / **nomic-embed-text-v2-moe** (multilingual, ~100
  languages incl. Polish) or **BGE-M3** for the semantic-retrieval/
  context-selection job — a separate model class from the generative LLM,
  to be benchmarked independently.

### Key risk: structured-output quality is size- and schema-shape dependent

A 2026 structured-output benchmark found JSON non-compliance rates jump
from 2-3% on flat schemas to **68-69% when schemas use `$defs`/nested
refs**, and that below ~4B params, compliance failures (not content
errors) dominate. **Design implication:** keep extraction schemas flat
wherever possible, and always use Ollama's grammar-constrained JSON-schema
mode rather than relying on prompting alone — this matters more than model
choice at this size class.

### Shortlist to benchmark (not a final pick — nothing downloaded/tested yet)

1. **Qwen3-4B-Instruct-2507** — best general-purpose candidate: modern,
   Apache 2.0, huge context, strong tool/JSON track record, fully resides
   in 4GB VRAM. Primary candidate for extraction/classification/
   summarization/context-selection.
2. **Bielik-4.5B-v3.0-Instruct** — the Polish-quality hedge. Given Polish
   is a hard requirement and general-purpose small models are
   inconsistently good at it, worth a head-to-head against #1 on Polish
   tasks specifically.
3. **Llama 3.1 8B Instruct** (CPU/partial-offload) — baseline/reference
   for "safest JSON schema compliance"; a slow-but-reliable fallback for
   less latency-sensitive batch jobs, using the abundant 64GB RAM.
4. **Gemma 3 4B** — multilingual breadth hedge, but budget time to build a
   prompt-engineering wrapper since raw structured extraction was reported
   weak without one.

Evaluate **GLiNER** separately for PII/redaction rather than assuming the
chat model should do it.

### Benchmark plan (to run before finalizing — no models downloaded as part of this research)

Setup: install Ollama, pull each shortlisted model at Q4_K_M, hit
`/api/generate`/`/api/chat` with `options.num_gpu` variations and `format`
set to JSON schema. Run each metric 5-10x, report p50/p95.

| Metric | Method |
|---|---|
| Startup/load time | Cold-start `ollama run <model>` to first prompt ready, separate from warm reload |
| Time-to-first-token | Stream API timestamps at short (200 tok) and long (4K tok) prompts |
| Tokens/sec (prefill + decode) | Ollama's own `eval_count/eval_duration`, `prompt_eval_count/prompt_eval_duration` |
| VRAM/RAM usage | `nvidia-smi --query-gpu=memory.used` + `ollama ps`, at multiple context lengths (1K/4K/16K) |
| JSON schema compliance | N=100-200 extraction prompts against 3-5 real LifeOS schemas (flat vs nested), score parse success + schema validity |
| Extraction accuracy | Hand-labeled 50-100 realistic LifeOS inputs, field-level precision/recall |
| Polish classification accuracy | ~100-200 Polish-language labeled inputs on LifeOS-relevant categories |
| English classification accuracy | Same task/template, English inputs, same taxonomy — to quantify the EN-PL gap per model |
| Redaction/PII-detection quality | Curated synthetic PII dataset (both languages); recall/precision, LLM-prompted vs GLiNER head-to-head |
| Context-selection quality | Small retrieval test set, precision@k/recall@k against a labeled gold set |

Suggested pass/fail gates: TTFT < 1.5s for interactive paths; decode ≥ 8
tok/s minimum viable; JSON schema compliance ≥ 95% (flat) / ≥ 85% (nested);
PII recall ≥ 98% (misses are the costly failure mode); PL-EN classification
gap ≤ 10pp. Benchmark Qwen3-4B and Bielik-4.5B first (cheapest to fully
test, most likely finalists) before the 8B/CPU-fallback path or Gemma's
wrapper investment.

### Sources

- https://willitrunai.com/can-run/llama-3.2-1b-on-rtx-3050-ti-laptop-4gb
- https://www.localscore.ai/accelerator/366 , https://www.localscore.ai/model/1
- https://localllm.in/blog/ollama-vram-requirements-for-local-llms
- https://localaimaster.com/vram/best-ollama-models-4gb-vram
- https://insiderllm.com/guides/structured-output-local-llms/
- https://arxiv.org/pdf/2604.25359 (structured-output benchmark)
- https://arxiv.org/pdf/2501.10868 (JSONSchemaBench)
- https://arxiv.org/pdf/2604.02367 (SLM front-door routing / Phi comparison)
- https://arxiv.org/pdf/2503.19786 (Gemma 3 technical report)
- https://qwenlm.github.io/blog/qwen2.5/ , https://huggingface.co/Qwen/Qwen3-4B-Instruct-2507
- https://arxiv.org/pdf/2501.02266 (LLMzSzŁ Polish benchmark) , https://huggingface.co/spaces/amu-cai/LLMZSZL_Leaderboard
- https://huggingface.co/speakleash/Bielik-4.5B-v3.0-Instruct-GGUF , https://ollama.com/SpeakLeash/bielik-4.5b-v3.0-instruct
- https://arxiv.org/abs/2511.03823 (PLLuM)
- https://github.com/urchade/GLiNER , https://huggingface.co/knowledgator/gliner-pii-small-v1.0
- https://github.com/meta-llama/llama-models/blob/main/models/llama3_2/LICENSE
- https://www.morphllm.com/ollama-embedding-models , https://ollama.com/library/nomic-embed-text-v2-moe

**Caveats:** Qwen3.5 small-variant specifics, the "Mistral Small 4"
naming, PLLuM's exact GGUF sizes/license, Phi-4-mini's benchmark score,
and any Polish score for Qwen3/Qwen3.5/Gemma3 could not be cross-confirmed
against a primary source and should be re-verified before eliminating a
candidate on their basis.

---

## 3. Voice / realtime conversation

Research goal: architecture direction for a future voice interface, given
LifeOS's brain is a cloud LLM (unlike NeXa, which builds its own local
realtime stack).

### NeXa reuse caveat

No live NeXa git repository exists on this machine — only Desktop
docx/txt planning notes referencing prior research into AEC3 echo
cancellation, self-echo suppression, and barge-in handling. There is no
NeXa source code available to mine for reuse in this environment; any
"NeXa lessons" here are concepts recalled from planning docs, not
validated, portable implementation.

### Central finding

The cascade (STT → LLM → TTS) is still the dominant production pattern in
2026, not native speech-to-speech, precisely because cascade keeps **one
brain** with full tool-calling, transcripts, and debuggability. Anthropic
itself has no native S2S API as of Sep 2026 — Claude Voice Mode is
cascaded under the hood. This matters directly for LifeOS: if the
domain-action layer is bound to a specific cloud LLM's tool-calling
schema, a provider-native S2S API is a **second, different brain** that
must be kept behaviorally consistent with the text-chat brain forever —
the opposite of "voice is just another interface to the same domain
layer."

### Comparison table

| Dimension | Pipecat (orchestration) | LiveKit Agents (orchestration) | Provider-native S2S (OpenAI Realtime/Gemini Live/Nova Sonic) | Cascaded best-of-breed (Deepgram/ElevenLabs + cloud LLM) |
|---|---|---|---|---|
| Latency | Depends on chosen services; low framework overhead | 200-500ms achievable via WebRTC | Best-in-class: 80-120ms (OpenAI), 100-200ms (Gemini) | 600ms-1.2s typical, sub-700ms achievable with fast providers |
| Cost/min | Framework free; pay providers plugged in | ~$0.038/min self-hosted-lean, or Cloud tier fees | OpenAI ~$0.06/$0.24 in/out per min; Gemini Live ~$0.005/$0.018; Nova Sonic ~$0.015/min (cheapest) | ~$0.10/min all-in (Deepgram+GPT-4o-mini+ElevenLabs Flash) |
| Barge-in/interruption | Native — Silero VAD + interruption propagation built in | Native, WebRTC-grade | Model-native, but requires client-side `response.cancel` + buffer flush (real gotcha — buffered audio can leak) | You own it — VAD + interrupt-and-flush must be built explicitly |
| Voice quality | Whatever TTS plugged in (ElevenLabs-grade available) | Same | High, small fixed voice set per provider | Best/most flexible — ElevenLabs Flash v2.5 (~75ms) is SOTA |
| Turn detection | Silero VAD + configurable endpointing | VAD + WebRTC signaling | Built-in server VAD + semantic/hybrid (Gemini) | STT-vendor endpointing (Deepgram Flux, ElevenLabs Scribe v2 RT <150ms) |
| Cross-platform desktop (Tauri) fit | Good — has a non-WebRTC/WebSocket transport, runs as a local process | Weaker — built around a WebRTC SFU for network transport a single local mic/speaker doesn't need | Fine — plain HTTPS/WebSocket call from Rust/Node, no framework needed | Fine — simplest surface area |
| Provider lock-in | Low — 15+ swappable STT/LLM/TTS integrations | Low for framework; LiveKit Cloud adds infra lock-in if used | High — proprietary model/format/tool-calling per vendor | Low — every stage independently swappable |
| Privacy | Neutral (depends on providers chosen) | Neutral | Audio leaves device to vendor's realtime endpoint; ZDR needs enterprise approval (OpenAI), Gemini retains session state up to 24h unless `store:false` | Same audio-leaves-device reality, but retention controllable per-stage |
| Implementation complexity | Medium (Python runtime to bundle in a Rust/Tauri app) | Medium-high (media-server ops burden not needed for local-only) | Low integration effort, but a second persona/tools to maintain | Medium — most moving parts, each simple and independently testable |
| License | BSD-2-Clause, Daily-maintained, active | Apache-2.0, LiveKit-maintained, active | Proprietary APIs | N/A (mix of proprietary provider APIs) |

### Decision: plan around a cascaded architecture, prototype with Pipecat

**STT → LifeOS's existing cloud-LLM brain (same tool/domain-action
bindings as text chat) → TTS**, prototyped with Pipecat's non-WebRTC
transport mode rather than LiveKit Agents, with a hand-rolled Rust audio
front-end (`cpal` capture/playback + local VAD) as the likely long-term
hot path once proven.

Why: keeps exactly one brain/persona/tool-binding set across text and
voice (the stated architectural requirement); every stage is independently
swappable and debuggable (transcripts, WER, latency per stage — important
where correctness on calendar/tasks/health data matters more than shaving
the last 200ms); avoids lock-in to one vendor's proprietary realtime audio
protocol; matches what Anthropic itself does for Claude Voice Mode and the
2026 production-voice-agent consensus for tool-heavy assistants; avoids
LiveKit's WebRTC SFU complexity for a network-transport problem LifeOS
doesn't have. Native S2S is logged as a future, optional, secondary mode
(e.g. low-stakes ambient chat with minimal tool surface), not the default.

### What to prototype/benchmark before committing (all in `LIFEOS_SPEC.md` §8)

End-to-end cascade latency under real tool-calling load; the Windows
speaker-to-mic self-echo/AEC path (`cpal` + WASAPI loopback +
`webrtc-audio-processing` Rust crate); Tauri webview audio-capture
reliability on Linux (WebKitGTK); Pipecat-as-Python-sidecar vs. hand-rolled
Rust cascade; local Silero VAD vs. cloud STT endpointing; privacy/retention
audit per chosen vendor; cost modeling per DAU-minute.

### Sources

- https://deepgram.com/learn/voice-agent-architecture-stt-llm-tts-pipeline-design
- https://github.com/pipecat-ai/pipecat
- https://soniox.com/wiki/voice-agent-frameworks
- https://docs.livekit.io/agents/ , https://www.cekura.ai/blogs/livekit-pricing
- https://openai.com/index/introducing-gpt-realtime/ , https://www.layer3labs.io/guides/openai-realtime-api-pricing
- https://ai.google.dev/gemini-api/docs/live-api/capabilities , https://rywalker.com/research/gemini-live-api
- https://docs.aws.amazon.com/bedrock/latest/userguide/model-card-amazon-nova-sonic.html , https://rywalker.com/research/aws-nova-2-sonic
- https://futureagi.com/blog/speech-to-text-apis-in-2026-benchmarks-pricing-developer-s-decision-guide/
- https://burki.dev/blog/43-deepgram-vs-elevenlabs-voice-ai
- https://www.coval.ai/blog/speech-to-speech-vs-cascaded-voice-ai-which-architecture-should-you-deploy/
- https://openai.com/index/offering-zero-data-retention-for-frontier-models/ , https://ai.google.dev/gemini-api/docs/zdr
- https://sqmagazine.co.uk/anthropic-claude-voice-mode-models/
- https://www.forasoft.com/learn/audio-for-video/articles-audio/acoustic-echo-cancellation-aec3
- https://github.com/tauri-apps/tauri/discussions/8426 (Tauri WebRTC on WebKitGTK)

---

## 4. Desktop stack, local database, notifications

Research goal: evaluate the desktop shell, DB encryption/key storage, and
native OS integration, factoring in the actual measured dev-machine state
(see `PROJECT_STATE.md`).

### 4a. Desktop shell

| Project | License | Maintenance (2026) | Strengths | Weaknesses | Verdict |
|---|---|---|---|---|---|
| **Tauri 2** | MIT/Apache-2.0, Commons Conservancy governance | Active; funded engineering (CrabNebula) | 20-50x smaller bundles (~3-10MB vs Electron 120-200MB); idle RAM ~42MB vs ~168MB (~75% less); cold start ~380ms vs ~1420ms; capability-based IPC security (frontend has no system access until explicitly granted) — much smaller attack surface than Electron's default Node access from renderer; also targets iOS/Android from the same codebase | Requires Rust + platform C compiler/linker (MSVC on Windows) — **on this machine the VS "Desktop development with C++" workload is missing and must be installed first**; smaller plugin ecosystem than npm; native webview rendering differences across OS need cross-platform testing | **REUSE** |
| Electron | MIT, OpenJS Foundation | Very mature, huge ecosystem | Enormous tooling maturity, consistent rendering, best-known auto-update tooling | 120-200MB installers, ~168MB idle RAM, slower cold start; renderer has full Node access by default (bigger attack surface without deliberate sandboxing); heavier long-term Chromium/Node patch burden | **REJECT** as primary — credible fallback only if Rust adoption proves infeasible |
| Wails (v2 stable/v3 alpha) | MIT | v2 stable, v3 maturing | Small bundles (10-30MB), simple Go backend | Go, not Rust — sacrifices the Rust-ecosystem crates (rrule, keyring, sqlite3mc bindings) this stack relies on | **REJECT** |
| Neutralino | MIT | Active but niche | Smallest bundles (2-5MB), no compile step | Limited native API surface, weak tray/notification/secure-storage depth needed here | **REJECT** |
| Flutter Desktop | BSD-3-Clause, Google | GA, production-ready | True native rendering, mobile-desktop code sharing | Dart instead of TS/React — discards the entire planned frontend ecosystem (RHF/zod/Recharts/FullCalendar); ~30-50MB empty-app footprint | **REJECT** |

**Critical prerequisite gap:** MSVC toolchain for Rust on Windows needs
Visual Studio Build Tools with the "Desktop development with C++" workload
(link.exe, Windows SDK, UCRT). This machine has VS 2022 Community
installed **without** that workload — confirmed via `vswhere` (no
component `Microsoft.VisualStudio.Component.VC.Tools.x86.x64` present).
Rust itself is also not installed yet. Both are concrete first setup
steps, not architectural blockers. Sources:
https://rust-lang.github.io/rustup/installation/windows-msvc.html ,
https://learn.microsoft.com/en-us/windows/dev-environment/rust/setup

### 4b. Local DB encryption

| Option | License | Maintenance | Strengths | Weaknesses | Verdict |
|---|---|---|---|---|---|
| SQLCipher | BSD-style community edition | Mature since 2008, slower cadence | Industry-standard, widely audited, AES-256-CBC+HMAC | Lower runtime perf than modern AEAD ciphers; Rust/Node bindings require building OpenSSL/libcrypto alongside SQLite (heavier Windows build); community-edition feature lag vs commercial | ADAPT (viable, but sqlite3mc is the better default) |
| **sqlite3mc (SQLite3 Multiple Ciphers)** | MIT | Active, closely tracks upstream SQLite | 6 cipher schemes incl. **ChaCha20-Poly1305 (AEAD, default, faster than AES-CBC)**; wire-compatible with SQLCipher's format (can read SQLCipher DBs); no external crypto library dependency; simpler Windows build (no OpenSSL linkage) | Younger name recognition, though functionally a superset | **REUSE** — recommended default |
| Application-level field encryption | N/A | N/A | Fine-grained, lets some columns stay queryable | Cannot encrypt indexes/schema/metadata; reinvents key management/nonce handling; larger attack surface for subtle crypto bugs | REJECT as primary — only for a specific extra-sensitive field later if ever needed |
| OS-level disk encryption (BitLocker/FileVault/LUKS) | Built into OS | N/A | Protects against physical theft when powered off, zero app code | **Windows 11 Home has no full BitLocker** — only simplified "Device Encryption," requires a Microsoft account and is off by default on local accounts (this machine's actual state); doesn't protect data when logged in/unlocked; doesn't protect a copied-out DB file | ADAPT — complementary layer, not a substitute |

**Decision:** `sqlite3mc` with ChaCha20-Poly1305 as the primary encryption
layer, not SQLCipher — actively maintained, faster, format-compatible if
migration is ever needed, no external OpenSSL dependency to fight on
Windows/MSVC. OS disk encryption layered underneath as defense-in-depth.

### 4c. Encryption-key storage

| Option | License | Maintenance | Strengths | Weaknesses | Verdict |
|---|---|---|---|---|---|
| **`keyring` Rust crate** | MIT/Apache-2.0 | Active, standard cross-platform crate | One crate, three backends automatically: Windows Credential Manager (DPAPI), macOS Keychain, Linux D-Bus Secret Service; no separate master password needed; well-understood | Linux backend depends on a running secret-service daemon (fine on GNOME/KDE, could matter on minimal/headless Linux — unlikely for a personal desktop) | **REUSE** |
| Tauri Stronghold plugin | MIT/Apache-2.0 | **Confirmed being deprecated** — a Tauri maintainer stated in the plugins-workspace discussion that Stronghold is no longer recommended and will be removed in v3 | Cross-platform-consistent vault format | Chicken-and-egg vault-password problem vs. a direct OS keystore; community sentiment moving to `keyring`; **dying dependency** | **REJECT** — do not build on this |
| DPAPI direct (Windows-only) | Built into Windows | N/A | No dependency at all | Windows-only — exactly the lock-in the brief asks to avoid | REJECT (use via `keyring` abstraction instead) |

**Decision:** the `keyring` crate, not Stronghold — a direct, evidenced
deviation from what the brief's candidate stack implied. Source:
https://github.com/orgs/tauri-apps/discussions/7846 (Stronghold
deprecation confirmation), https://github.com/open-source-cooperative/keyring-rs

### 4d. SQLite driver, migrations, future vector search

| Option | License | Maintenance | Strengths | Weaknesses | Verdict |
|---|---|---|---|---|---|
| **rusqlite** (+ sqlite3mc via feature/custom `libsqlite3-sys`) | MIT | Very active, the standard sync Rust SQLite binding | Simple, synchronous (fine for a local single-user DB), supports bundling sqlite3mc/SQLCipher via feature flags; community Tauri SQL plugins already wire this pattern | Official `tauri-plugin-sql` (sqlx-based) lacks first-class SQLCipher/sqlite3mc support (open upstream issues) | **REUSE** (via rusqlite directly in Tauri commands, not the official sqlx-based plugin) |
| sqlx | MIT/Apache-2.0 | Active, async-first | Async, compile-time-checked queries, used by the official `tauri-plugin-sql` | No native sqlite3mc integration without overriding `libsqlite3-sys`; async adds complexity with no real benefit for a local single-user app | REJECT for this project |
| **refinery** | MIT/Apache-2.0 | Active | Directly supports `rusqlite`, migrations as plain `.sql` files or versioned Rust modules, simple embed-in-binary workflow | Smaller community than sqlx's built-in migrator (non-issue, SQLite-only project) | **REUSE** — pairs with rusqlite |
| **sqlite-vec** | MIT/Apache-2.0 | Active — designated successor to sqlite-vss | Pure C, zero external dependencies, runs anywhere SQLite runs, sized for thousands-to-millions of vectors (matches personal-app scale) | Newer, less mature ANN tuning than dedicated vector DBs (non-issue at this scale) | **REUSE (future)** — for later semantic memory/search |
| sqlite-vss | — | **Not in active development** — author moved to sqlite-vec | Was the pioneer | Explicitly deprecated | REJECT |

### 4e. Calendar/recurrence

| Option | License | Maintenance | Strengths | Weaknesses | Verdict |
|---|---|---|---|---|---|
| **rrule.js** | MIT | Mature, widely used (also powers FullCalendar) | Battle-tested RFC5545 in JS/TS, directly usable in the React calendar UI | None significant at this scale | **REUSE** (frontend recurrence expansion) |
| **`rrule` Rust crate** (fmeringdal/rust-rrule) | MIT/Apache-2.0 | Active | Pure Rust RFC5545 (RRuleSet: RRULE+EXRULE+RDATE+EXDATE), ideal for backend scheduling that must work when the UI isn't open | Smaller community than rrule.js (expected for Rust) | **REUSE** (backend recurrence for reminders/notifications) |
| rrule-rust (npm wrapper over the Rust crate) | — | Maintained | Single source of truth for both sides | Adds a native-binding build step | ADAPT — only if frontend/backend recurrence divergence becomes a real bug source |

**Decision:** rrule.js on the frontend, the Rust `rrule` crate on the
backend — two independent, RFC-identical implementations rather than a
native binding, matching the brief's "don't hand-roll RFC5545" directive.

### 4f. Notifications & background execution (Tauri)

| Capability | Mechanism | Windows | Linux/macOS notes | Verdict |
|---|---|---|---|---|
| Notifications | `tauri-plugin-notification` (official) | Native Windows toast | macOS Notification Center; Linux via libnotify/desktop portal | REUSE |
| Autostart | `tauri-plugin-autostart` (official) | Windows startup entry | macOS Login Items; Linux XDG `.desktop` autostart | REUSE |
| Tray/background | Tauri core tray APIs | Tray + `skip_taskbar` to stay resident | macOS tray next to menu bar; **Linux requires `libayatana-appindicator`/`libappindicator3`** installed | REUSE, Linux dependency flagged for later |
| Auto-updater | `tauri-plugin-updater` (official) | Signed NSIS/MSI | DEB/RPM/AppImage (Linux), DMG (macOS) — genuinely cross-platform | REUSE |
| Single-instance guard | `tauri-plugin-single-instance` (official) | Standard pairing with tray+autostart | Cross-platform | REUSE |

### 4g. Frontend UI

| Layer | Candidate | License | Strengths | Weaknesses | Verdict |
|---|---|---|---|---|---|
| Forms/validation | **React Hook Form + Zod** | MIT | Uncontrolled-input performance, Zod schemas double as TS types (single source of truth) | None significant | REUSE |
| Forms alt. | TanStack Form | MIT | Framework-agnostic, good typing | Smaller ecosystem/precedent, no compelling reason to deviate | ADAPT (fallback only) |
| Charting | **Recharts (v3)** | MIT | 2.4M weekly downloads, simple declarative JSX, solid TS types, covers standard dashboard chart types | ~150KB bundle (acceptable — not bundle-constrained like a web page) | REUSE |
| Charting alt. | visx | MIT | Full control, ~15KB core primitives | Steep learning curve, overkill for standard dashboard needs | ADAPT (only for a specific chart Recharts can't express) |
| Charting alt. | Tremor | Apache-2.0 core | Fastest to a good-looking dashboard, built on Recharts | Less flexible/more opinionated styling | ADAPT (if a shadcn/ui design system is adopted) |
| Calendar UI | **react-big-calendar** | MIT, no paid tier | 100% free, all features available, integrates naturally with rrule.js | Less polished out-of-the-box than FullCalendar Premium (e.g. resource views) | REUSE |
| Calendar UI alt. | FullCalendar | MIT core / commercial Premium ($480/dev/yr for premium views) | Extremely polished, most feature-complete | Core free tier lacks some premium views; ongoing cost for premium in a personal project | ADAPT — start with free core if sufficient |

### Overall desktop-stack decision

Tauri 2 + React + TypeScript + Rust + **sqlite3mc** (not SQLCipher) +
**`keyring`** (not Stronghold) + rusqlite + refinery + rrule.js/rrule-rs +
React Hook Form/Zod + Recharts + react-big-calendar, with sqlite-vec
reserved for future semantic search and Ollama retained for local-LLM/
voice work. This is a **partial, evidence-based deviation** from the
brief's candidate stack (SQLCipher and — implicitly — Stronghold) on the
two points above; everything else in the candidate stack holds up against
current evidence.

Evaluated against stated criteria (Windows-first/Linux-macOS-ready,
security, privacy, installer/updater, notifications, background work,
future voice, maintainability, performance, dev speed, community/
licensing): every REUSE-verdict item is MIT or MIT/Apache-2.0 dual
licensed and under active 2026 maintenance, cross-platform by
construction, with the one flagged future action item being the Linux
tray-icon dependency (`libayatana-appindicator`).

### Immediate setup action items (also tracked in `PROJECT_STATE.md`)

1. Install the "Desktop development with C++" workload in the existing VS
   2022 Community installer (MSVC v143 build tools + Windows SDK + UCRT).
2. Install Rust via `rustup` (not yet present).
3. Do not reach for `tauri-plugin-stronghold` or the official sqlx-based
   `tauri-plugin-sql` — use `rusqlite` (sqlite3mc bundled) + `refinery` +
   `keyring` directly.

### Sources

- https://rustify.rs/articles/rust-tauri-vs-electron-2026
- https://v2.tauri.app/blog/tauri-20/ , https://tauri.app/about/governance/
- https://v2.tauri.app/blog/partnership-crabnebula/
- https://github.com/utelle/SQLite3MultipleCiphers/
- https://utelle.github.io/SQLite3MultipleCiphers/docs/ciphers/cipher_sqlcipher/
- https://pureinfotech.com/enable-bitlocker-device-encryption-windows-11-home/
- https://github.com/open-source-cooperative/keyring-rs
- https://v2.tauri.app/plugin/stronghold/ , https://github.com/orgs/tauri-apps/discussions/7846
- https://github.com/razein97/tauri-plugin-rusqlite2
- https://github.com/tauri-apps/plugins-workspace/issues/7 , /issues/2528
- https://github.com/rust-db/refinery
- https://github.com/asg017/sqlite-vec , https://github.com/asg017/sqlite-vss
- https://github.com/fmeringdal/rust-rrule
- https://v2.tauri.app/release/notification/v2.0.0/ , https://v2.tauri.app/plugin/autostart/
- https://v2.tauri.app/learn/system-tray/ , https://v2.tauri.app/plugin/updater/
- https://stacknotice.com/blog/react-hook-form-zod-guide-2026
- https://www.pkgpulse.com/guides/recharts-v3-vs-tremor-vs-nivo-react-charting-2026
- https://bryntum.com/blog/react-fullcalendar-vs-big-calendar/

---

## 5. Existing personal-AI / second-brain OSS projects

Research goal: find reusable architecture, components, or ideas from
existing open-source personal-AI/life-management projects.

### Candidate comparison

| Project | License | Maintenance | Core purpose | Relevance | Verdict |
|---|---|---|---|---|---|
| **Khoj** (khoj-ai/khoj) | **AGPL-3.0** | Active, 37.4k★ | AI second brain: chat over docs, agents, scheduled automations | Best analog for the AI chat/action layer + multi-LLM routing + document memory design. Wrong stack (Django/Postgres/Next.js web app) and AGPL blocks direct code reuse | **ADAPT** (ideas only) |
| **Leon AI** (leon-ai/leon) | **MIT** | Active, 17.5k★, 2.0 in dev preview | Open-source personal assistant, voice-capable, agentic | **Skills → Actions → Tools → Functions** hierarchy with three execution modes (smart/controlled/agent); layered memory model. MIT removes reuse friction but 2.0 is unstable and the stack (Node) doesn't match a Rust backend | **ADAPT** (study the action hierarchy and memory layering) |
| Reor (reorproject/reor) | AGPL-3.0 | **ARCHIVED Mar 7, 2026**, no successor | Local AI note app, RAG-based Q&A over notes | Local embeddings + LanceDB + Ollama + markdown-as-source-of-truth pattern worth studying; unmaintained and Electron-based | REJECT for reuse / ADAPT for ideas only |
| **AFFiNE** (toeverything/affine) | MIT (core) + separate Enterprise license (backend EE) | Very active, 72.7k★ | Notion/Miro-style all-in-one workspace | Strongest **local-first sync/CRDT architecture reference** — `y-octo` (Rust CRDT) / OctoBase, explicit "you always own your data on disk" philosophy. Too heavyweight to reuse wholesale; avoid the EE-licensed backend pieces | **ADAPT** (study the CRDT design for future multi-device sync) |
| Logseq | AGPL-3.0 (core) | Active | Outliner/journal-based PKM | Block-based, bi-directionally-linked, dated blocks with TODO/DONE state — good reference for LifeOS's journal/problem-tracking domain model. Clojure/Electron stack and AGPL make code reuse impractical | ADAPT (data-model shape) / REJECT for code |
| **Wealthfolio** (wealthfolio/wealthfolio) | **AGPL-3.0** | Active, ~9k★, 704 forks | Local-first personal finance/portfolio tracker | **Closest stack analog overall**: Tauri + Rust + SQLite (Diesel) + React/TS, optional Axum server, encrypted multi-device sync, addon/plugin SDK with sandboxing/permissions. Directly relevant for the money module and, more importantly, its **crate separation by domain** (core, storage-sqlite, market-data, connect, device-sync) is a strong template for LifeOS's own domain boundaries. AGPL means a direct code copy would force LifeOS itself to be AGPL — a real decision point | **ADAPT strongly** (architecture/crate layout/plugin SDK pattern); reference-only unless LifeOS accepts AGPL |
| Manuel0516/second-brain | MIT | Low-traction (0★, 118 commits), hobby-stage but working | Self-hosted life OS: calendar, notes, fitness, food, AI assistant | Closest **conceptual** domain match to LifeOS. The **"AI assistant works through the same guarded/typed API as the UI"** pattern is a genuinely useful privacy/architecture principle, carried directly into `LIFEOS_SPEC.md` §2 | **ADAPT** (guarded-API-for-AI pattern) |
| zulucode-design/second-brain (fork of HelixNotes) | AGPL-3.0-or-later | Early alpha, active | PARA-method knowledge app with local AI | **Tauri 2 + SvelteKit + Rust** reference: Tantivy full-text search, Syncthing sidecar for sync over Tailscale, Ollama for semantic search, "keyword search always works, AI is additive" resilience principle | ADAPT (AI-optional resilience design, sidecar-sync idea) |
| GsirGinRay/second-brain-workspace | MIT | Pre-release/internal | Local-first task/project manager | Markdown-file storage (no DB) with atomic writes/crash-recovery, `.ai/INDEX.md` for AI discoverability. Too immature and wrong storage model for LifeOS's structured domains (finance, mood scores) | REJECT (note the `.ai/INDEX.md` idea only) |
| OpenHuman (tinyhumansai/openhuman) | License unconfirmed — verify before use | Active, heavily marketed | "Personal AI super intelligence": persistent memory, agent orchestration | The **"Memory Tree"** pattern — SQLite-backed, scored/compressed memory mirrored to human-readable Markdown/Obsidian vault — directly relevant to LifeOS's long-term memory subsystem. Overall scope (agent fleets, 5,000+ MCP servers) is far beyond LifeOS's needs and looks over-engineered | ADAPT (scored-memory-mirrored-to-markdown pattern); verify license before going further |
| Monica | AGPL-3.0 | Active, in transition | Personal CRM (relationships/contacts) | Only tangential (no goals/finance/journal/mood); relevant only if a future relationships module is added | REJECT for now |
| Generic hobby "life-os" repos (various) | Mostly MIT | Low-traction, early | Personal life dashboards | Confirms the product category exists; browser-storage-only, no native desktop layer, no usable architecture | REJECT |

### AGPL flag — important for LifeOS licensing decisions

Most of the strongest analogs (Khoj, Reor, Wealthfolio, Monica, Logseq
core, zulucode-design/second-brain) are **AGPL-3.0**. If LifeOS is
distributed as closed-source or under a different license, none of that
code can be copied or linked without triggering AGPL's network-copyleft
obligations — they can only be studied for ideas. The MIT-licensed
candidates (Leon AI, AFFiNE core, Manuel0516/second-brain,
GsirGinRay/second-brain-workspace) are the only ones where actual code
reuse would be legally uncomplicated, but none are close enough in
domain+stack to justify it — they're architecture references too. **Net
effect: no candidate found is a REUSE-as-a-library case for LifeOS. Every
verdict is ADAPT (ideas) or REJECT.** LifeOS's own license (not yet
decided) should be chosen independently of any temptation to lift AGPL
code — this is an explicit open item.

### Architectural lessons carried into LIFEOS_SPEC.md

1. **Guarded API as the AI's only interface** (Manuel0516/second-brain) →
   `LIFEOS_SPEC.md` §2.
2. **Skills → Actions → Tools → Functions layered command hierarchy**
   (Leon AI 2.0) → informs the action-layer design in `LIFEOS_SPEC.md` §2.
3. **AI-optional resilience** (zulucode-design/second-brain) →
   `LIFEOS_SPEC.md` §2.
4. **Rust crate separation by domain** (Wealthfolio) → `LIFEOS_SPEC.md`
   §5.1.
5. **Scored memory mirrored to human-readable Markdown** (OpenHuman) →
   `LIFEOS_SPEC.md` §5.1.
6. **CRDT-based local-first sync** (AFFiNE's y-octo/OctoBase) → noted as
   the v2+ multi-device sync direction, `LIFEOS_SPEC.md` §13.
7. **Sidecar sync via Syncthing** (zulucode-design/second-brain) → noted
   as a pragmatic interim alternative to CRDT, `LIFEOS_SPEC.md` §13.
8. **Block/outliner data model for journal + problem/solution tracking**
   (Logseq) → informs the domain-model direction in `LIFEOS_SPEC.md` §5.1.

**Bottom line:** no existing OSS project is a drop-in fit for LifeOS's
exact combination (Tauri 2 + Rust + SQLite + structured
goals/money/calendar/wellbeing/journal domains + local-first with optional
cloud LLM chat). Wealthfolio is the closest stack-level analog and the
single best architecture reference despite AGPL blocking direct reuse.
Reor's archival (Mar 2026) is a useful cautionary data point — even a
well-regarded local-first AI notes app in this exact space folded, a risk
factor worth remembering for LifeOS's own scope discipline
(`CLAUDE.md`, "Scope discipline").

### Sources

- https://github.com/khoj-ai/khoj
- https://github.com/leon-ai/leon
- https://github.com/reorproject/reor
- https://github.com/toeverything/affine
- https://github.com/logseq
- https://github.com/wealthfolio/wealthfolio
- https://github.com/Manuel0516/second-brain
- https://github.com/zulucode-design/second-brain
- https://github.com/GsirGinRay/second-brain-workspace
- https://github.com/tinyhumansai/openhuman
- https://github.com/monicahq/monica
