---
description: Create a standalone interactive HTML explainer in the Konductor for Amazon EventBridge editorial style
argument-hint: '[topic, source files, or output path]'
---

# Create Explainer

Create a standalone interactive HTML explainer for `$ARGUMENTS`.

Use this command when the user wants a shareable narrative page that explains a technical change, architecture decision, incident, data flow, or product behavior.

## Tone and voice

Explainers are educational documents, not marketing pages. Write in plain, direct English that a new team member or external reviewer can follow without prior context.

- **No hype.** Avoid words like "revolutionary", "seamless", "effortless", "powerful", "unlock", "leverage", "transform", "unprecedented", "cutting-edge", "next-generation". If a sentence sounds like it could appear on a landing page, rewrite it.
- **No editorial theatrics.** Avoid rhetorical questions, dramatic paragraph openers ("Imagine a world where..."), and fake urgency ("the stakes could not be higher"). Start with the fact.
- **Concrete before abstract.** State what the system does, then why it matters. Never the reverse.
- **Short sentences.** Break complex thoughts into two sentences. One idea per paragraph.
- **Name things by what they do.** Use "the sync engine queues pending changes" not "our resilient synchronization layer orchestrates outbound mutations". Prefer function names and file paths over invented proper nouns.
- **Educational, not promotional.** The reader should learn something they can use. Every section should leave them better informed than they started.

## What's in here

A self-contained kit for one explainer — copy what the topic needs, ignore the rest:

- **First steps** — the two questions to ask before drafting.
- **Design language** — the banned→replacement table, the device set, and tokens. Read this first; it is what separates the refined explainers from generated-looking ones.
- **Component library** — copy-paste CSS/HTML for every device (ledgers, inspect panels, flags, circuits, coverage maps, dot plots, annotated geometry), extracted from the worked example.
- **Content structure** — a default outline, plus a ready-made **arc for refactoring / architecture explainers**.
- **Interaction patterns** — steppers, animated diagrams, live demos, and the writing principles that keep them clear.
- **Present mode** — the built-in slide deck every explainer ships with, and the rules for composing its slides.
- **Verification & final response.**

Worked references: `explainers/engineering-foundation-explainer.html` for the design language and every device below; `explainers/services-internal-architecture-explainer.html` for Present mode, steppers, and live demos.

## First steps

Before writing anything, use the **AskUser tool** to ask two focused questions in one call:

1. Ask whether relevant source material already exists — check `openspec/` and `site/docs/` first, then ask:
   - "Is this documented in `openspec/` or `site/docs/`? Or would you rather describe it directly?"
   - "Do you have any screenshots or UI recordings to include? (Attach them, or say no.)"

Keep the questionnaire to those two questions. Do not ask about audience, tone, or outline — infer those from the topic and produce a first draft. The user will steer from there.

If the user says source files exist, scan them before writing. If they attach images, embed them as base64 data URIs (see **Screenshots** below).

After gathering that input:

- If the user did not provide an output path, save to `explainers/<topic>-explainer.html`.
- Do not commit the generated explainer unless the user explicitly asks.

## Design language

Every explainer ships at the bar set by **`explainers/engineering-foundation-explainer.html`** — the canonical design-language reference. Read it before building. When a rule below is unclear, copy what that file does; do not invent. (`explainers/services-internal-architecture-explainer.html` is the reference for Present mode and steppers.)

The look is editorial, not dashboard: deep navy canvas, one cyan accent, hairline rules, mono microtype, generous whitespace. A finished page is **typography + hairlines + dots + the device set below** — nothing else. These rules exist so the bar is reachable mechanically, without taste calls: follow them literally.

### Banned → replacement

Hard rules, not taste. If a draft contains anything in the left column, replace it before showing the user.

| Never | Always instead |
|---|---|
| Emoji as iconography (🛡️ 🐹 🔗) | Mono step numbers (`01`…), status dots, plain typography |
| Bordered or tinted boxes ("cards") holding text | Grouped ledger + inspect panel; aside columns for short prose |
| Status pills, badges, lozenges, chips | `.flag` — 5px dot + mono uppercase microtext |
| Tinted background fills as status | Hairline border; **dashed = not yet**; **accent = selected / the subject** |
| Progress bars | Dot plot on one labelled qualitative spectrum |
| Boxed `<table>` | Ledger grid — mono microlabel header row + hairline rows |
| Text crammed inside SVG shapes | Empty geometry + hairline leader lines to outside labels |
| Hover-only or cursor-following tooltips | Anchored popover (hover + tap + focus) or a fixed detail panel |
| Centered multi-line text in containers | Left-aligned |
| Fixed-width step boxes; >6-step horizontal pipelines | Numbered ledger rows, or a journey track |
| A coloured mark on every item | Mark exceptions only; state the default once ("everything here is active unless flagged") |
| Pill-shaped filter tabs | Mono text rail with a funnel icon, dim counts, accent underline on active |
| Every diagram element in the same colour | One accent for subject/selection; dashed for absent/future; everything else quiet |
| Repeating in a diagram a label the prose already established | Diagrams carry signal, not echo |

**Exemptions:** interactive *controls* (play buttons, segmented controls, theme/present toggles) and *live-demo surfaces* (sim panes, payload/record panels) may keep borders and backgrounds — they are machinery, not content.

### The device set

Compose pages from these. Combining is fine; inventing beyond them needs a reason, and the invention must still be hairlines + mono microtype + dots.

1. **Grouped ledger** — the default for any list, table, or grid of ≥5 items. One-line rows under mono group headers.
2. **Inspect panel (master–detail)** — when ledger items or diagram segments carry prose. Hover/tap/focus selects; the panel swaps.
3. **Annotated geometry** — empty SVG shapes, labels leader-lined outside (the test pyramid).
4. **Node circuit / journey track** — sequences and cycles: numbered circles on a hairline; dashed return path for loops; vertical gate bars crossing a path for checkpoints.
5. **Butterfly coverage map** — two subjects guarding shared concerns; empty wing = honest absence.
6. **Dot plot** — scores or maturity as dots on one labelled spectrum, never bars.
7. **Flags & dots** — the page's only status grammar.
8. **Aside / ruled columns** — short prose and checklists behind a hairline left rule.

### Tokens

Dark (default):

```css
:root {
  color-scheme: dark;
  --bg:#06060a; --surface:#0e0e14; --surface-hover:#14141c;
  --border:#1e1e2e; --border-strong:#2a2a40;
  --text:#e8e8f0; --text-dim:#7a7a8c;
  --text-faint:#62627a; /* decorative/microtype only — not body text */
  --accent:#00d9ff; --accent-dim:#00d9ff33;
  --success:#00e676; --success-dim:#00e67622;
  --error:#ff5252; --error-dim:#ff525222;
  --warning:#ffd740; --warning-dim:#ffd74022;
  --violet:#b388ff; --violet-dim:#b388ff22;
  --mono:"SF Mono","Fira Code",ui-monospace,SFMono-Regular,Menlo,Monaco,Consolas,monospace;
  --sans:"Inter",-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif;
  --ease-out-expo:cubic-bezier(0.16,1,0.3,1); --ease-in-out:cubic-bezier(0.65,0,0.35,1);
}
[data-theme="light"] {
  color-scheme: light;
  --bg:#faf9f7; --surface:#ffffff; --surface-hover:#f5f4f2;
  --border:#e8e6e3; --border-strong:#d4d0ca;
  --text:#0a0a0f; --text-dim:#6b6b7b; --text-faint:#8e8e9a;
  --accent:#0066ff; --accent-dim:#0066ff18;
  --success:#00c853; --success-dim:#00c85318;
  --error:#d50000; --error-dim:#d5000018;
  --warning:#ffab00; --warning-dim:#ffab0018;
  --violet:#6200ea; --violet-dim:#6200ea18;
}
```

Verify both themes — `--*-dim` colours that read on navy can vanish on paper-white.

### Typography & layout

- Hero headline: `clamp(3rem, 8.5vw, 7.5rem)`, weight 800, letter-spacing `-0.04em`, line-height `0.92`.
- Section heads: `clamp(1.7rem, 3.5vw, 3rem)`, weight 700. Section subs: `--text-dim`, max-width `60ch`.
- Body: `1rem`, line-height `1.7`, `--text-dim`, max-width `60ch`. Detail text in devices: `.8–.92rem`.
- Microtype (eyebrows, group headers, flags, stamps): `var(--mono)`, `.58–.68rem`, uppercase, `.12–.16em` tracking.
- Shell: `max-width:1100px`, `padding:0 clamp(24px,5vw,80px)` — **content width ≈ 956px at full size. Before sizing any horizontal device, do the arithmetic: items × min-width + gaps must fit 956px, or it silently clips.**
- Grid blowout guard on every multi-column grid: `> * { min-width: 0 }`.
- Breakpoints: 900px (multi-column → single, tracks go vertical, leaders hide) and 720px (hero collapses, ledger headers hide, rows stack). Always include `@media (prefers-reduced-motion: reduce)` disabling all animation.
- Sticky side panels: `position: sticky; top: 48px`, released to `static` under 900px.
- **Vertical rhythm is hierarchical:** ~40px binds a section heading to its first block; **72px** (`.artifact{margin-top:72px}`) separates sibling artifacts inside a section; and every artifact opens with its own mono eyebrow (`Why the gaps exist`, `The gap ledger · 6 open`) so multi-artifact sections read as titled chapters, not piles. In Present mode, override artifacts back to the stage rhythm (`.pr-stage .artifact{margin-top:28px!important}`).
- **End the page with a bookend** — one `muted-note` line after the final artifact that echoes the hero claim ("That's the picture — further along than it feels, with every gap numbered"). A page that stops on a ledger reads truncated.

### Motion

- Scroll reveal: IntersectionObserver on `.reveal` → `opacity 0 / translateY(50px)` → visible over `.9s var(--ease-out-expo)`, staggered `.reveal-delay-1…6` (0.06s steps).
- Panel swap: re-trigger a single `pyrIn`-style keyframe (`opacity 0 / translateY(6px)`, .3s) by toggling a `.swap` class and forcing reflow (`void el.offsetWidth`).
- Two fixed radial-gradient orbs, `pointer-events:none`, slow float. Theme toggle: fixed 44px circle, top-right; Present toggle beside it.

## Component library

Copy-paste blocks, extracted verbatim from the worked example — if these drift from `engineering-foundation-explainer.html`, the example wins.

*Base & shell:*

```css
*{box-sizing:border-box;margin:0;padding:0}
html{scroll-behavior:smooth;-webkit-font-smoothing:antialiased}
body{background:var(--bg);color:var(--text);font-family:var(--sans);font-size:16px;line-height:1.6;overflow-x:hidden}
.shell{max-width:1100px;margin:0 auto;padding:0 clamp(24px,5vw,80px);position:relative;z-index:1}
section{padding:clamp(72px,10vh,130px) 0;position:relative}
section+section::before{content:'';position:absolute;top:0;left:clamp(24px,5vw,80px);right:clamp(24px,5vw,80px);height:1px;background:linear-gradient(90deg,transparent,var(--border-strong) 20%,var(--border-strong) 80%,transparent)}
.eyebrow{font-family:var(--mono);font-size:.68rem;text-transform:uppercase;letter-spacing:.15em;color:var(--text-dim)}
.block-head{display:flex;justify-content:space-between;align-items:baseline;gap:16px;flex-wrap:wrap;margin-bottom:18px}
.data-stamp{font-family:var(--mono);font-size:.6rem;letter-spacing:.12em;text-transform:uppercase;color:var(--text-faint)}
.muted-note{margin-top:10px;font-size:.82rem;color:var(--text-dim)}
.artifact{margin-top:72px}
```

`block-head` pairs an eyebrow with a right-aligned `data-stamp`. **Stamp every hand-counted dataset** (file tallies, subjective scores): `counts as of 11 Jun 2026`. Honest beats evergreen-looking.

*Hero* — kicker, display headline (`em` for the accent word), lede, optional glossary hint. No chips, no badges:

```html
<div class="hero-kicker">Project · Topic</div>
<h1 class="display">Short<br>punchy <em>claim</em></h1>
<p class="lede">One- or two-sentence thesis.</p>
<p class="gloss-hint">New to the stack? <button class="term" data-term="gloss">Dotted terms</button> open a plain-English definition.</p>
```

*Status flags & dots — the only status grammar:*

```css
/* Status flags — dot + mono microtext, the only status grammar on the page */
.flag{display:inline-flex;align-items:center;gap:7px;font-family:var(--mono);font-size:.62rem;letter-spacing:.12em;text-transform:uppercase;white-space:nowrap}
.flag i{width:5px;height:5px;border-radius:50%;flex-shrink:0}
.flag.done{color:var(--success)}.flag.done i{background:var(--success)}
.flag.progress{color:var(--warning)}.flag.progress i{background:var(--warning)}
.flag.next{color:var(--accent)}.flag.next i{background:var(--accent)}
.flag.gap{color:var(--error)}.flag.gap i{background:var(--error)}
```

Usage: `<span class="flag done"><i></i>also in CI</span>`. **Mark exceptions, not the default** — if 17 of 22 items are active, active rows carry nothing; only ◐ partial and ○ upcoming rows get a flag, and the section sub states the default once. Same flip everywhere (e.g. all steps run locally → flag only the ones *also* in CI).

*Glossary terms (for readers new to the domain):*

Mark jargon once per neighbourhood; define in one `GLOSSARY` map ({term:{t:title,d:plain-English definition}}); a single shared popover element. Open on click (delegated — terms also appear in JS-rendered panels), close on Esc / outside-click / scroll; position centered under the button, clamped to the viewport, flipped above when out of room; `aria-expanded` on the open term.

```css
/* Glossary terms */
.term{appearance:none;background:none;border:0;padding:0;font:inherit;color:inherit;cursor:help;text-decoration:underline dotted color-mix(in srgb,var(--accent) 60%,transparent);text-underline-offset:3px;text-decoration-thickness:1px}
.term:hover,.term:focus-visible,.term[aria-expanded="true"]{color:var(--accent);text-decoration-style:solid;outline:none}
.gloss-pop{position:absolute;z-index:240;width:min(330px,calc(100vw - 32px));background:var(--surface);border:1px solid var(--border-strong);border-radius:12px;padding:16px 18px;box-shadow:0 12px 40px rgba(0,0,0,.35);opacity:0;pointer-events:none;transition:opacity .2s ease,transform .2s ease;transform:translateY(4px)}
.gloss-pop.open{opacity:1;pointer-events:auto;transform:none}
.gloss-pop-term{font-family:var(--mono);font-size:.68rem;letter-spacing:.12em;text-transform:uppercase;color:var(--accent);margin-bottom:8px}
.gloss-pop-body{font-size:.84rem;color:var(--text-dim);line-height:1.6}
.gloss-pop-body a{color:var(--accent)}
```

*Grouped ledger + filter rail + inspect panel* — the workhorse. List rows render from a data array; selection (mouseenter, click, focus, Enter/Space) drives the panel; the filter rail shows/hides groups:

```css
/* What's in place — grouped scan list + sticky inspect panel */
.filter-rail{display:flex;gap:20px;flex-wrap:wrap;align-items:center;margin-bottom:24px;font-family:var(--mono);font-size:.65rem;letter-spacing:.14em;text-transform:uppercase}
.filter-rail-icon{color:var(--text-faint);flex-shrink:0;margin-bottom:5px}
.filter-rail button{appearance:none;background:none;border:0;padding:0 0 5px;font:inherit;letter-spacing:inherit;text-transform:inherit;color:var(--text-dim);cursor:pointer;border-bottom:1px solid transparent;transition:all .25s ease}
.filter-rail button:hover{color:var(--text)}
.filter-rail button.active{color:var(--accent);border-bottom-color:var(--accent)}
.filter-rail button span{opacity:.5;margin-left:3px}
.inspect-legend{display:inline-flex;gap:22px;margin-left:auto;margin-bottom:5px;font-family:var(--mono);font-size:.62rem;letter-spacing:.12em;text-transform:uppercase;color:var(--text-dim);flex-wrap:wrap}
.inspect-legend span{display:inline-flex;align-items:center;gap:8px}
.dot{width:8px;height:8px;border-radius:50%;flex-shrink:0}
.dot.active{background:var(--success)}
.dot.partial{background:linear-gradient(90deg,var(--warning) 50%,transparent 50%);border:1px solid var(--warning)}
.dot.upcoming{border:1px solid var(--accent);background:transparent}
.inspect-grid{display:grid;grid-template-columns:minmax(0,1fr) minmax(280px,.8fr);gap:56px;align-items:start}
.inspect-list{min-width:0}
.inspect-group{font-family:var(--mono);font-size:.62rem;letter-spacing:.16em;text-transform:uppercase;color:var(--text-faint);padding:28px 0 10px;border-bottom:1px solid var(--border)}
.inspect-list .inspect-group:first-child{padding-top:0}
.inspect-row{display:flex;align-items:center;gap:14px;padding:11px 2px;border-bottom:1px solid var(--border);cursor:pointer;outline:none}
.inspect-row-name{font-size:.92rem;color:var(--text);font-weight:400;line-height:1.4;transition:color .25s ease}
.inspect-row.flagship .inspect-row-name{font-weight:600}
.inspect-row-flag{margin-left:auto;padding-left:12px;display:inline-flex;align-items:center;gap:7px;font-family:var(--mono);font-size:.58rem;letter-spacing:.12em;text-transform:uppercase;white-space:nowrap}
.inspect-row-flag.partial{color:var(--warning)}
.inspect-row-flag.upcoming{color:var(--accent)}
.inspect-row:hover .inspect-row-name,.inspect-row:focus-visible .inspect-row-name,.inspect-row.sel .inspect-row-name{color:var(--accent)}
.inspect-side{position:sticky;top:48px}
.inspect-detail{border-left:1px solid var(--border-strong);padding-left:28px}
.inspect-eyebrow{font-family:var(--mono);font-size:.65rem;letter-spacing:.14em;text-transform:uppercase;margin-bottom:10px}
.inspect-eyebrow.active{color:var(--success)}
.inspect-eyebrow.partial{color:var(--warning)}
.inspect-eyebrow.upcoming{color:var(--accent)}
.inspect-title{font-size:1.15rem;font-weight:700;margin-bottom:10px;color:var(--text);line-height:1.4}
.inspect-body{font-size:.88rem;color:var(--text-dim);line-height:1.7}
.inspect-body code{font-family:var(--mono);font-size:.85em;color:var(--accent)}
.inspect-pills{margin-top:18px;display:flex;gap:16px;flex-wrap:wrap}
.inspect-detail.swap>*{animation:pyrIn .3s var(--ease-out-expo)}
```

```html
<div class="filter-rail" id="filters">
  <svg class="filter-rail-icon" width="13" height="13" viewBox="0 0 16 16" aria-hidden="true"><path d="M1.5 2.5h13L10 8.6V13l-4 1.8V8.6L1.5 2.5z" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"/></svg>
  <button class="active" data-key="all">All <span>22</span></button>
  <button data-key="quality">Code Quality <span>6</span></button>
  <span class="inspect-legend"><!-- rides the rail line, right-aligned -->
    <span><i class="dot partial"></i>partially in place</span>
    <span><i class="dot upcoming"></i>upcoming</span>
  </span>
</div>
<div class="inspect-grid">
  <div class="inspect-list" id="list"><!-- rendered from data --></div>
  <div class="inspect-side" id="side">
    <div class="inspect-detail" id="detail" aria-live="polite">
      <div class="inspect-eyebrow"></div><h4 class="inspect-title"></h4>
      <p class="inspect-body"></p><div class="inspect-pills"></div>
    </div>
  </div>
</div>
```

JS contract (see the worked example for the full version):

- Row: `tabindex=0`, `role=button`; flagship items get `font-weight:600` and bolder meaning is explained in the section sub.
- `select(id)`: toggle `.sel`, fill eyebrow (`Category · Status`, class-coloured), title (may contain `term` buttons), body (innerHTML), flags; re-trigger `.swap`; close any open glossary popover.
- `filter(key)`: show/hide groups; if the selected row becomes hidden, select the filtered group's first item; selecting a hidden row programmatically resets the filter to All.
- **Mobile:** on selection under 900px, move the panel node directly *under the selected row* (accordion feel); restore it to the side column above 900px (and while presenting). `resize` re-places it.

*Header ledgers (replaces every `<table>`):*

```css
/* De-boxed ledger grids (gaps, bigger picture) */
.ledger-head{display:grid;gap:24px;padding:0 2px 10px;border-bottom:1px solid var(--border);font-family:var(--mono);font-size:.62rem;letter-spacing:.16em;text-transform:uppercase;color:var(--text-faint)}
.ledger-row{display:grid;gap:6px 24px;padding:13px 2px;border-bottom:1px solid var(--border);align-items:baseline}
.gaps-cols{grid-template-columns:minmax(0,1.5fr) minmax(0,.55fr) minmax(0,.62fr) 64px}
.compare-cols{grid-template-columns:minmax(0,.75fr) minmax(0,1fr) minmax(0,1.15fr)}
.ledger-title{font-size:.92rem;font-weight:600;color:var(--text)}
.ledger-sub{font-size:.82rem;color:var(--text-dim);line-height:1.5;margin-top:2px}
.ledger-sub code{font-size:.85em;font-family:var(--mono);color:var(--accent)}
.ledger-dim{font-size:.85rem;color:var(--text-dim)}
.ledger-flags{display:flex;flex-direction:column;gap:8px;align-items:flex-start}
.ledger-flags .flag{white-space:normal;text-align:left}
```

Comparison usage: dim the baseline column (`.ledger-dim`) and let the subject column carry colour via flags — the baseline literally looking grey is the argument. On mobile the header row hides; give orphaned dim cells a mono `::before` prefix (`typical:`).

*Numbered sequence ledger (pipelines, ordered steps):*

```css
/* make mr — numbered gate ledger; order reads top-to-bottom */
.gate-list{margin-top:8px}
.gate-row{display:grid;grid-template-columns:36px 176px minmax(0,1fr) auto;gap:6px 16px;align-items:baseline;padding:10px 2px;border-bottom:1px solid var(--border)}
.gate-num{font-family:var(--mono);font-size:.62rem;letter-spacing:.12em;color:var(--text-faint)}
.gate-name{font-family:var(--mono);font-size:.78rem;font-weight:700;color:var(--text)}
.gate-desc{font-size:.85rem;color:var(--text-dim);line-height:1.5}
```

```html
<div class="gate-row">
  <span class="gate-num">01</span><span class="gate-name">lint</span>
  <span class="gate-desc">oxlint → ESLint + security plugin</span>
  <span class="flag done"><i></i>also in CI</span><!-- exceptions only -->
</div>
```

*Link ledger (roadmaps, grouped external links):*

```css
/* What's next — grouped ledger of epic links */
.rm-group{display:flex;align-items:center;gap:9px;font-family:var(--mono);font-size:.62rem;letter-spacing:.16em;text-transform:uppercase;color:var(--text-faint);padding:28px 0 10px;border-bottom:1px solid var(--border)}
.rm-list .rm-group:first-child{padding-top:0}
.rm-group i{width:6px;height:6px;border-radius:50%;flex-shrink:0}
.rm-group.now i{background:var(--warning)}
.rm-group.beta i{background:var(--accent)}
.rm-group.ga i{background:var(--violet)}
.rm-row{display:flex;align-items:baseline;gap:16px;padding:12px 2px;border-bottom:1px solid var(--border);text-decoration:none}
.rm-row-title{font-size:.92rem;font-weight:600;color:var(--text);white-space:nowrap;transition:color .25s ease}
.rm-row-desc{font-size:.82rem;color:var(--text-dim);line-height:1.5;min-width:0}
.rm-row-ref{margin-left:auto;padding-left:12px;font-family:var(--mono);font-size:.62rem;letter-spacing:.1em;color:var(--text-faint);white-space:nowrap;transition:color .25s ease}
.rm-row:hover .rm-row-title,.rm-row:hover .rm-row-ref{color:var(--accent)}
```

Group headers carry a coloured phase dot (`<div class="rm-group now"><i></i>Now · 2</div>`); rows are `<a>` with title / desc / `Epic #25 ↗` ref. An item with no tracker yet is a `<div>` row whose ref is `<span class="flag next"><i></i>Epic TBD</span>` — the visual reminder to create it.

*Aside & ruled-emphasis columns (short prose, checklists, warnings):*

```css
/* Editorial aside columns — hairline rule instead of boxes */
.pair-grid{display:grid;grid-template-columns:1fr 1fr;gap:44px;align-items:start}
.pair-grid>*{min-width:0}
.aside-col{border-left:1px solid var(--border-strong);padding-left:24px}
.aside-col .eyebrow{display:block;margin-bottom:14px}
.aside-col p{font-size:.9rem;color:var(--text-dim);line-height:1.65}
.aside-col p em{color:var(--text)}
.check-list{list-style:none;display:grid;gap:10px}
.check-list li{display:flex;gap:10px;font-size:.9rem;color:var(--text-dim)}
.check-list li>span{flex-shrink:0}
.check-list .yes{color:var(--success)}
.check-list .meh{color:var(--text-faint)}
```

`gap-why` is the replacement for tinted callout boxes: warning-ruled columns, `h3` in the warning colour, body in `--text-dim`.

*Dot plot (replaces every progress bar):*

```css
/* Workflow maturity — dot plot on a single qualitative spectrum */
.maturity-grid{display:grid;grid-template-columns:minmax(0,.9fr) 1fr;gap:4px 32px;align-items:center;margin-top:8px}
.maturity-ends{grid-column:2;display:flex;justify-content:space-between;font-family:var(--mono);font-size:.58rem;letter-spacing:.12em;text-transform:uppercase;color:var(--text-faint);margin-bottom:4px}
.mat-label{font-size:.9rem;color:var(--text);padding:9px 0;line-height:1.45}
.mat-label code{font-size:.82em;font-family:var(--mono)}
.mat-track{position:relative;height:24px}
.mat-track::before{content:'';position:absolute;left:0;right:0;top:50%;height:1px;background:var(--border)}
.mat-track::after{content:'';position:absolute;right:0;top:50%;transform:translateY(-50%);width:1px;height:9px;background:var(--border-strong)}
.mat-tick{position:absolute;left:0;top:50%;transform:translateY(-50%);width:1px;height:9px;background:var(--border-strong)}
.mat-dot{position:absolute;top:50%;left:0;transform:translate(-50%,-50%);width:9px;height:9px;border-radius:50%;transition:left 1.1s var(--ease-out-expo)}
.mat-dot.full{background:var(--success)}
.mat-dot.high{background:var(--accent)}
.mat-dot.low{background:var(--error)}
```

Axis ends are qualitative (`not started` → `strong`), never numeric — invented percentages read as false precision. Dots animate `left: 0 → data-width%` on reveal (and on slide entry in Present mode). Pair with a `data-stamp`.

*Node circuit (loops) — numbered nodes on a hairline track, dashed return path, detail panel below:*

```css
/* Loop diagram — numbered nodes on a circuit, detail panel below */
.loop-track{position:relative;display:flex;justify-content:space-between;margin:18px 0 0}
.loop-track::before{content:'';position:absolute;left:8.3%;right:8.3%;top:18px;height:1px;background:var(--border-strong)}
.loop-node{position:relative;flex:1;display:flex;flex-direction:column;align-items:center;gap:11px;cursor:pointer;outline:none}
.loop-node i{width:37px;height:37px;border-radius:50%;border:1px solid var(--border-strong);background:var(--bg);display:grid;place-items:center;font-family:var(--mono);font-style:normal;font-size:.62rem;letter-spacing:.06em;color:var(--text-dim);transition:all .25s ease}
.loop-node:hover i,.loop-node:focus-visible i{border-color:var(--text-dim);color:var(--text)}
.loop-node.sel i{border-color:var(--accent);color:var(--accent);box-shadow:0 0 0 4px var(--accent-dim)}
.loop-node.future i{border-style:dashed}
.loop-node-label{font-size:.8rem;font-weight:600;color:var(--text);text-align:center;line-height:1.35;max-width:124px;transition:color .25s ease}
.loop-node.future .loop-node-label{color:var(--text-dim)}
.loop-node.sel .loop-node-label{color:var(--accent)}
.loop-detail{border-left:1px solid var(--border-strong);padding-left:28px;margin-top:40px;min-height:96px}
.loop-detail-eyebrow{font-family:var(--mono);font-size:.65rem;letter-spacing:.14em;text-transform:uppercase;margin-bottom:8px;color:var(--success)}
.loop-detail-eyebrow.future{color:var(--warning)}
.loop-detail-title{font-size:1.05rem;font-weight:700;margin-bottom:6px;color:var(--text)}
.loop-detail-body{font-size:.88rem;color:var(--text-dim);line-height:1.7;max-width:64ch}
.loop-detail-body code{font-family:var(--mono);font-size:.85em;color:var(--accent)}
.loop-detail.swap>*{animation:pyrIn .3s var(--ease-out-expo)}
.loop-return{position:relative;height:24px;margin:14px 8.3% 0;border:1px dashed var(--border-strong);border-top:none;border-radius:0 0 14px 14px}
.loop-return-arrow{position:absolute;left:-5.5px;top:-10px;color:var(--text-faint);font-size:.8rem;line-height:1}
.loop-return-label{position:absolute;left:50%;bottom:0;transform:translate(-50%,50%);background:var(--bg);padding:0 14px;font-family:var(--mono);font-size:.62rem;letter-spacing:.16em;text-transform:uppercase;color:var(--text-faint);white-space:nowrap}
```

Nodes render from a data array (`{label, body, future?}`); `i` holds the mono number; `.future` nodes get a dashed circle and a warning eyebrow in the panel; the selected node gets the accent ring. The dashed `.loop-return` runs under the row with its label sitting **on** the line (solid `--bg` background so the line doesn't slice the text) and an `↑` into node 1. Under 900px the track turns vertical and the return hides.

*Journey / gauntlet track (checkpoints on a path):* a horizontal SVG line with station dots and labels above (`working tree → commit → push → MR → main`), gates as short vertical bars crossing it, each labelled below in mono microtext with what runs there. Active gates solid `--success`; the section's subject taller in `--accent`; a partial gate dashed `--warning`. One such diagram absorbs a whole "X runs first, then Y" paragraph. CSS:

```css
/* Gate flow — a change's path from working tree to main, through four gates */
.gate-flow svg{width:100%;max-width:960px;display:block}
.gf-line{stroke:var(--border-strong)}
.gf-station{font-family:var(--sans);font-size:12.5px;font-weight:600;fill:var(--text)}
.gf-dot{fill:var(--text-faint)}
.gf-name{font-family:var(--mono);font-size:9.5px;font-weight:700;letter-spacing:.08em}
.gf-detail{font-family:var(--mono);font-size:9px;fill:var(--text-dim)}
```

*Butterfly coverage map (two subjects, shared concerns):*

```css
/* Linter coverage map — concerns on a center spine, oxlint left, golangci right */
.lint-map{margin-top:72px}
.lm-intro{font-size:.9rem;color:var(--text-dim);line-height:1.7;max-width:60ch;margin-bottom:36px}
.lm-head,.lm-row{display:grid;grid-template-columns:1fr 210px 1fr;gap:0 18px;align-items:center}
.lm-head{align-items:baseline;margin-bottom:22px}
.lm-h{display:flex;flex-direction:column;gap:4px}
.lm-h code{font-family:var(--mono);font-size:.95rem;font-weight:700;color:var(--accent)}
.lm-h.right{align-items:flex-end;text-align:right}
.linter-scope{font-family:var(--mono);font-size:.6rem;letter-spacing:.12em;text-transform:uppercase;color:var(--text-faint)}
.lm-mid-label{text-align:center;font-family:var(--mono);font-size:.58rem;letter-spacing:.16em;text-transform:uppercase;color:var(--text-faint)}
.lm-row{min-height:46px}
.lm-cell{display:flex;align-items:center;gap:14px;font-size:.8rem;color:var(--text-dim);line-height:1.45}
.lm-cell code{font-family:var(--mono);font-size:.92em;color:var(--accent)}
.lm-left{justify-content:flex-end;text-align:right}
.lead{flex:1 0 26px;height:1px;background:var(--border)}
.lm-concern{text-align:center;font-family:var(--mono);font-size:.64rem;letter-spacing:.12em;text-transform:uppercase;color:var(--text-dim);line-height:1.5}
```

Row = left wing (right-aligned text + `.lead` hairline) / center `.lm-concern` / right wing. **Leave a wing empty where coverage genuinely doesn't exist** — the asymmetry is the signal — and explain why in a one-line `muted-note`. Open the map with an eyebrow that ties it to its context ("Steps 01 and 07, expanded") plus a one-sentence reading key. Mobile: rows stack, concern first, wings get `ts`/`go` mono prefixes, empty wings hide.

*Annotated geometry (the test-pyramid pattern):*

```css
/* Test pyramid — annotated geometry left, detail panel right */
.pyr-grid{display:grid;grid-template-columns:minmax(0,1.1fr) minmax(250px,.9fr);gap:44px;align-items:center}
.pyr-grid svg{width:100%;max-width:560px;display:block}
.pyr-row{cursor:pointer;outline:none}
.pyr-shape{fill:var(--text);fill-opacity:.04;stroke:var(--border-strong);stroke-width:1;transition:all .25s ease}
.pyr-row:hover .pyr-shape,.pyr-row:focus-visible .pyr-shape{stroke:var(--text-dim)}
.pyr-row.sel .pyr-shape{stroke:var(--accent);fill:var(--accent);fill-opacity:.07}
.pyr-name{font-family:var(--sans);font-size:12.5px;font-weight:600;fill:var(--text);transition:fill .25s ease}
.pyr-row.sel .pyr-name{fill:var(--accent)}
.pyr-count{font-family:var(--mono);font-size:9.5px;fill:var(--text-dim)}
.pyr-leader{stroke:var(--border-strong);stroke-width:1}
.pyr-flag{font-family:var(--mono);font-size:8.5px;letter-spacing:.08em}
.pyr-detail{border-left:1px solid var(--border-strong);padding-left:28px}
.pyr-detail-eyebrow{font-family:var(--mono);font-size:.65rem;letter-spacing:.14em;text-transform:uppercase;margin-bottom:10px}
.pyr-detail-eyebrow.ci{color:var(--success)}
.pyr-detail-eyebrow.local{color:var(--warning)}
.pyr-detail-title{font-size:1.15rem;font-weight:700;margin-bottom:6px;color:var(--text)}
.pyr-detail-meta{font-family:var(--mono);font-size:.72rem;color:var(--text-dim);margin-bottom:14px}
.pyr-detail-body{font-size:.88rem;color:var(--text-dim);line-height:1.7}
.pyr-detail-body a{color:var(--accent)}
.pyr-detail.swap>*{animation:pyrIn .3s var(--ease-out-expo)}
@keyframes pyrIn{from{opacity:0;transform:translateY(6px)}to{opacity:1;transform:none}}
```

Rules: shapes are **empty** (1px `--border-strong` stroke, 4–7% fill); names + counts hang off hairline leaders on one side, status dot + microflag on the other; selecting a layer (hover, tap, Tab+Enter — `<g tabindex="0" role="button">`) turns it accent and swaps the `border-left` detail panel beside it. Compute leader endpoints so labels never collide; widest label decides the viewBox margin. Copy the SVG geometry from the worked example and adjust coordinates rather than deriving from scratch.

*TL;DR toggle:* native `<details>` reveal-upward summary pill — keep from previous explainers (see services-internal). Pill-shaped **buttons** are fine; pill-shaped *status* is not.

*Micro-ledgers:* `fact-grid` (dt/dd rows), `timeline-item` rows, and `data-row` key-value rows from earlier explainers remain acceptable — they are already hairline rows. `issue-link` (bordered CTA link) is a control, exempt.

*Interactive-demo machinery (exempt from the no-boxes rule):* segmented controls, `.play-btn`, sim panes, payload/record panels — keep the patterns from `services-internal-architecture-explainer.html` / `local-first-architecture-explainer.html` for live simulations.

*Responsive (paste and extend per device used):*

```css
@media(max-width:900px){
  .pair-grid{grid-template-columns:1fr}
  .lm-head{grid-template-columns:1fr 1fr}
  .lm-mid-label{display:none}
  .lm-row{grid-template-columns:1fr;gap:7px;min-height:0;padding:12px 0;border-bottom:1px solid var(--border)}
  .lm-concern{text-align:left;order:-1}
  .lead{display:none}
  .lm-left{justify-content:flex-start;text-align:left}
  .lm-cell:empty{display:none}
  .lm-left::before{content:'ts';font-family:var(--mono);font-size:.58rem;letter-spacing:.1em;text-transform:uppercase;color:var(--text-faint)}
  .lm-right::before{content:'go';font-family:var(--mono);font-size:.58rem;letter-spacing:.1em;text-transform:uppercase;color:var(--text-faint)}
  .loop-track{flex-direction:column;align-items:stretch;gap:18px}
  .loop-track::before{left:18px;right:auto;top:18px;bottom:18px;width:1px;height:auto}
  .loop-node{flex-direction:row;gap:14px}
  .loop-node-label{text-align:left;max-width:none}
  .loop-return{display:none}
  .gate-row{grid-template-columns:36px minmax(0,1fr) auto}
  .gate-num{grid-row:1;grid-column:1}
  .gate-name{grid-row:1;grid-column:2}
  .gate-row .flag{grid-row:1;grid-column:3}
  .gate-desc{grid-row:2;grid-column:2/4}
  .pyr-grid{grid-template-columns:1fr;gap:28px}
  .pyr-detail{padding-left:20px}
  .inspect-grid{grid-template-columns:1fr}
  .inspect-side{position:static}
  .inspect-detail{margin:14px 0 22px;padding-left:20px}
  .gap-why{grid-template-columns:1fr}
  .compare-cols{grid-template-columns:1fr}
  #pictureTable .ledger-head{display:none}
  .compare-cols .ledger-dim::before{content:'typical: ';font-family:var(--mono);font-size:.58rem;letter-spacing:.1em;text-transform:uppercase;color:var(--text-faint)}
  .maturity-grid{grid-template-columns:1fr;gap:2px}
  .maturity-ends{grid-column:1}
  .mat-label{padding:12px 0 0}
}
@media(max-width:720px){
  .hero-grid{grid-template-columns:1fr}
  .hero-note{display:none}
  .rm-row{flex-direction:column;gap:3px}
  .rm-row-title{white-space:normal}
  .rm-row-ref{margin-left:0;padding-left:0}
  .ledger-head{display:none}
  .ledger-row{grid-template-columns:1fr}
}
@media(prefers-reduced-motion:reduce){
  *,*::before,*::after{animation:none!important;transition:none!important}
  .reveal{opacity:1;transform:none}
}
```

Present-mode zoom guard: every responsive collapse above needs a matching `.pr-stage` override restoring the desktop layout inside the deck (CMD− shrinks the CSS viewport and would otherwise collapse slides to mobile layouts). See the worked example's `.pr-stage` block.

**Code blocks:**

CSS — use these exact values. `pre-wrap` causes bad wrapping in narrow columns; `pre` + `overflow-x: auto` gives a scrollbar instead:

```css
.code-block pre {
  padding: 20px 16px;
  font-family: var(--mono);
  font-size: 0.80rem;
  line-height: 1.75;
  color: var(--text-dim);
  white-space: pre;
  overflow-x: auto;
}
```

**JSON highlighting** — include these CSS classes, then run `highlightCode()` on JSON blocks:

```css
.json-key    { color: var(--accent); }
.json-string { color: #98c379; }
.json-number { color: #d19a66; }
.json-bool   { color: #c678dd; }
.json-null   { color: var(--text-dim); }
```

**Go highlighting** — use a token-based `highlightGo()` that extracts comments and string literals into `\x00N\x00` placeholders *before* running keyword/type/number passes. This prevents the regex from matching inside span attribute values (e.g. `"go-str"` getting re-wrapped as a string token and corrupting the markup):

```js
function highlightGo(text) {
  let html = text.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');
  const tokens = [];
  const tok = (cls, m) => { const i = tokens.length; tokens.push(`<span class="${cls}">${m}</span>`); return `\x00${i}\x00`; };
  // 1. Comments and strings first (highest priority — never re-processed)
  html = html.replace(/(\/\/[^\n]*)/g, m => tok('go-com', m));
  html = html.replace(/(\/\*[\s\S]*?\*\/)/g, m => tok('go-com', m));
  html = html.replace(/(`[^`\x00]*`)/g, m => tok('go-str', m));
  html = html.replace(/("(?:[^"\\\x00]|\\.)*")/g, m => tok('go-str', m));
  // 2. Keywords, types, numbers
  const kws = ['package','import','func','type','struct','interface','map','return','if','else','for','range','switch','case','default','var','const','go','defer','select','chan','nil','true','false'];
  html = html.replace(new RegExp(`\\b(${kws.join('|')})\\b`,'g'), '<span class="go-kw">$1</span>');
  const types = ['string','int','int64','bool','error','any','byte','float64','context','Context','len','make','new','append'];
  html = html.replace(new RegExp(`\\b(${types.join('|')})\\b`,'g'), '<span class="go-type">$1</span>');
  // Exclude digits adjacent to the \x00 token placeholders — otherwise this pass
  // wraps the placeholder indices, the restore step fails to match, and bare numbers
  // (0, 1, 2…) leak into the rendered code where comments/strings should be.
  html = html.replace(/(?<!\x00)\b(\d+(?:\.\d+)?)\b(?!\x00)/g, '<span class="go-num">$1</span>');
  html = html.replace(/\bfunc\b\s+(\w+)\s*\(/g, m => m.replace(/(\w+)(?=\s*\()/, n => `<span class="go-fn">${n}</span>`));
  // 3. Restore placeholders
  return html.replace(/\x00(\d+)\x00/g, (_, i) => tokens[+i]);
}
```

**`highlightAll()`** — run once on init for static blocks. For any JS-rendered code block (dynamic payload panels, tab-switching), re-run `highlightAll()` after replacing the DOM content. Use `pre.textContent = code` + `highlightAll()` rather than `innerHTML = highlightCode(code)` directly, so the guard (`pre.querySelector('span[class^="go-"]')`) can detect already-processed blocks and skip them.

## Screenshots

If the topic has a UI surface, screenshots are the fastest way to give readers the "aha" moment. Place them **immediately after the hero text**, before any numbered sections.

**When the user attaches images:**

- Base64-encode each image and embed as a `data:image/png;base64,...` URI. The file must remain self-contained and shareable as a single HTML file.
- Use Python to encode: `base64.b64encode(open(path,'rb').read()).decode('ascii')`.

**Screenshot showcase layout:**

```html
<!-- Full-width main screenshot -->
<div class="screenshot-main reveal reveal-delay-2">
  <div class="screenshot-frame">
    <img src="data:image/png;base64,..." alt="..." loading="lazy">
  </div>
  <p class="screenshot-caption">Short caption.</p>
</div>

<!-- Optional 2-up pair for detail shots -->
<div class="screenshot-pair reveal reveal-delay-3">
  <div><div class="screenshot-frame"><img ...></div><p class="screenshot-caption">...</p></div>
  <div><div class="screenshot-frame"><img ...></div><p class="screenshot-caption">...</p></div>
</div>
```

**Screenshot CSS:**

```css
.screenshot-frame {
  border: 1px solid var(--border);
  border-radius: 16px;
  overflow: hidden;
  background: #0d0d18;
  box-shadow: 0 0 0 1px var(--border), 0 32px 80px -16px rgba(0,0,0,0.5);
  transition: box-shadow 0.4s var(--ease-out-expo), border-color 0.4s var(--ease-out-expo), transform 0.4s var(--ease-out-expo);
  position: relative;
}
.screenshot-frame::before {
  content: '';
  position: absolute; top: 0; left: 0; right: 0; height: 1px;
  background: linear-gradient(90deg, transparent 5%, var(--accent-dim) 40%, var(--accent-dim) 60%, transparent 95%);
  opacity: 0.6; z-index: 1;
}
.screenshot-frame:hover {
  border-color: var(--border-strong);
  transform: translateY(-4px);
  box-shadow: 0 0 0 1px var(--border-strong), 0 48px 100px -20px rgba(0,217,255,0.1);
}
.screenshot-frame img { display: block; width: 100%; height: auto; }
.screenshot-main { margin-bottom: 16px; }
.screenshot-pair { display: grid; grid-template-columns: 1fr 1fr; gap: 16px; }
.screenshot-caption { margin-top: 12px; font-family: var(--mono); font-size: 0.72rem; color: var(--text-dim); letter-spacing: 0.04em; }
```

If no screenshots are provided, skip the showcase section entirely. Do not use placeholder boxes.

## Content structure

Use this default outline unless the topic calls for something else:

1. **Hero** — one-sentence conclusion, 2–3 key chips, location in the product if it is a UI feature, and the screenshot showcase if images were provided.
2. **Issue / background** — why this exists or what was broken.
3. **Attempts** — why earlier approaches failed (toggle tabs).
4. **End-to-end flow** — stepper through the sequence.
5. **Interaction** — small live simulation of the key behavior with play/speed controls.
6. **Storage / data model** — tables, records, payloads, boundaries.
7. **What's next** — lifecycle, risks, open questions, issue link.

Compress or reorder sections freely when the topic does not need all of them. Narrative flow matters more than completeness.

### Refactoring / architecture explainer — reference skeleton

For "we restructured X" topics, this arc is a fast start — change or drop any section to taste; it is a head-start, not a mandate. Worked example: `explainers/services-internal-architecture-explainer.html`.

1. **Hero** — the one-line outcome + 2–3 chips, with a **TL;DR toggle** carrying "the one rule" and "what is unchanged" so the detail is opt-in.
2. **The problem** — what was tangled before, concretely; the thing the refactor kills.
3. **The rule** — the single organizing principle the refactor introduces. Name it plainly.
4. **Anatomy** — the rule applied to one unit, interactive (rail/tabs) so the reader watches the shape repeat.
5. **A concern moved to a boundary** — when the refactor extracts a responsibility (auth, transport, …): a two-zone **boundary diagram** (each zone's role + what crosses, labelled both ways), then a **"where it's headed"** future-architecture flow if that concern becomes its own service.
6. **Stitching it together** — how the pieces compose into the deliverable (binaries, wiring) as a short **stepper**.
7. **Walkthrough: adding the next thing** — the payoff. Work **backwards from a concrete goal** ("starting tomorrow, how do I add Templates?"), one step per place that must change, ending in a **mental-model table** (the few invariants × where each lives). Concrete example first, the why second.
8. **Appendix: guardrails** — how the structure is kept from drifting (deterministic lint vs judged review).

Narrative principles that make the difference:

- **Concrete before abstract.** Show the diff/code for the real example, then one line of why. A practical maintainer trusts a worked example over an elegant description.
- **Work backwards from the goal** in the "how to extend" walkthrough — start from what the reader wants to be true, then the steps that make it true.
- **End on a mental model** — a small table the reader can hold in their head, not a recap paragraph.
- **Name things by what they do.** Avoid jargon that over-carries meaning or trips ESL readers (we replaced "seam" with "boundary", "PEP/PDP" with "Role").
- **Keep the example consistent across steps.** If one step shows a typed constant and another shows a raw string for the same concept, say why (e.g. a boundary package that cannot import your types) or harmonize it — an unexplained mismatch reads as a mistake.
- **Diagrams carry signal, not echo** (see below).

## Interaction patterns

Pick interactions that make the topic easier to understand:

- **Animated architecture diagram** for any topic with a multi-step data flow (see pattern below).
- Stepper for sequence/data-flow explanations.
- Instant segmented controls instead of slow range sliders.
- Toggle tabs for failed attempts, alternatives, or before/after states.
- Small live payload/record panels for concrete data shape examples.
- DOM-append animations for step-by-step simulations (append rows, do not replace innerHTML).
- A dot plot on a qualitative spectrum for scores, maturity, or capacity comparisons (never bars).
- **Stepper for procedures and look-alike code.** When the topic is "how to add/extend X", or shows several similar constructors/presets/registry entries, use a click-through stepper (numbered dots + Prev/Next) that reveals ONE concept per step. Never dump a list of unexplained helpers and assume the reader infers the pattern.

### Progressive disclosure — minimise what is on screen

The reader should see only what the current step needs. A wall of code, or a table of look-alike entries (e.g. `direct` / `directReview` / `reviewScoped` / `templateScoped`), reads as ambiguous — it does not convey that they are the same shape reused, and it overloads the reader. Instead:

- **One concept per step.** Decompose a procedure into a stepper: what the thing is → pick the variant → show the variant is tiny → register it → test it → done. Each step is a short paragraph plus at most one small code block.
- **Explain every named helper the first time it appears.** If a snippet calls `templateScoped(...)`, a later step must show what `templateScoped` expands to and why it is reused — opaque names erode confidence that "it's the same thing per case."
- **Stepper mechanics:** default to step 1; disable Prev/Next at the ends; mark visited dots done; re-run `highlightAll()` after injecting each step's code (set `pre.textContent` then highlight, so the highlighter escapes it — never `innerHTML` raw code).
- Reach for a stepper over one long section whenever the content has ≥4 steps or ≥3 similar code variants. Trim explanatory prose hard — if a section reads as a wall of text next to the others, break it into facts/steps or cut it.

### Diagrams carry signal, not echo

A diagram is not a place to restate the prose. Do not repeat a label the surrounding text already established. If the lead-in says "own binary, own API, own scaling profile," the diagram's nodes must not also read "today's binary" / "later phase" — that is noise. Label nodes with what is *new* there: the names, the contents (stores, fields), the differentiator that makes the two sides behave or scale differently. The text states it once; the diagram shows it. (Diagram and connector labels that sit on a line need a solid `background: var(--surface)` so the line does not slice through the text.)

### Animated architecture diagram

Use this for hero sections that show a system with two or more components communicating. Replace the static SVG with a step-through sequence diagram.

**Pattern:**

1. Draw an SVG with named `<g class="seq-node" id="seqXxx">` groups for each component and `<g id="seqXxxFlow" class="seq-flow">` overlays for each arrow. Base arrows (static, dim) and animated flow overlays (hidden until activated) are separate elements.
2. The hero-grid right column becomes a dynamic annotation panel (`id="seqAnnot"`). On idle it shows the normal prose. On each step it fades to the step title + description.
3. Seven step dots (`class="seq-dot"`) sit below the SVG. The advance button cycles through steps and resets.
4. CSS for node dimming and flow animation:

```css
.seq-node { transition: opacity 0.45s ease, filter 0.45s ease; }
#seqSvg.seq-running .seq-node { opacity: 0.12; }
#seqSvg.seq-running .seq-node[data-sl] { opacity: 1; }
[data-sl="accent"]  { filter: drop-shadow(0 0 8px var(--accent));  }
[data-sl="warning"] { filter: drop-shadow(0 0 8px var(--warning)); }
[data-sl="success"] { filter: drop-shadow(0 0 8px var(--success)); }
.seq-flow { opacity: 0; transition: opacity 0.3s ease; pointer-events: none; }
.seq-flow.seq-flowing { opacity: 1; }
@keyframes seqDash { to { stroke-dashoffset: -28; } }
.seq-flowing .seq-dash { animation: seqDash 0.55s linear infinite; }
.seq-dot.sact { background: var(--accent); box-shadow: 0 0 0 3px var(--accent-dim); }
.seq-dot.sdone { background: var(--success); }
```

5. JS: a `seqSteps[]` array (step label, color `accent|warning|success`, title, desc), parallel `seqNodeMap[]` and `seqFlowMap[]` arrays, and a `showSeq(index)` function that sets `data-sl` on active nodes and `seq-flowing` class on active flows, then fades the annotation panel. `index = -1` resets to idle.

**SVG tips:**

- Use `class="seq-dash"` on lines inside flow groups so the `seqDash` animation applies.
- Route internal backend connections (e.g. API → PostgreSQL) along the right margin of the backend outer box so they don't visually overlap other component boxes.
- Always include base (static, dim) arrows so the diagram is readable before interaction starts.
- Internal arrows that span multiple sub-components (e.g. pg_notify going from PostgreSQL up to SSE) can be short — the step annotation carries the explanation.

Avoid:

- Carousels that hide core information.
- Animations where all nodes pulse at once.
- Interactions that require dragging to understand a discrete state.
- Decorative SVGs that reduce readability.
- **Hover-only or mouse-following tooltips.** Hover is dead on touch and invisible to keyboards, and a tooltip glued to the cursor jitters and clips. Anchor the popover to the hovered element (centered, clamped to its container, flipped above/below as needed) and open it on hover, **click/tap (pinned)**, and **focus** — give SVG targets `tabindex="0"` and an `aria-label`, close on Esc and outside-click.

## Present mode (required)

Every explainer ships with a presentation mode: a `▶` button beside the theme toggle (and the `P` key) turns the scroll page into a keyboard-driven slide deck. Use it to walk a room through the explainer without losing any of the page's interactivity.

**How it behaves:**

- `P` or the `▶` button enters the deck starting at the slide that corresponds to the current scroll position — so you can scroll to any section in narrative mode and press `P` to jump straight into it. `Esc` exits and restores the page exactly (scroll position included). `F` toggles fullscreen.
- `→` / `↓` / `Space` / `PageDown` advance — **fragments first, then the next slide**. `←` / `↑` / `PageUp` go back. `Home` / `End` jump.
- A pill HUD (prev / `n ⁄ N` / next) fades out after 3s idle and wakes on mouse move; a 2px accent progress hairline sits at the bottom.
- Slides **adopt the live DOM nodes** declared in their manifest and give them back on exit. Steppers, rails, play buttons, and theme toggling all keep working inside a slide — never duplicate content into the deck.
- An auto-fit pass scales each slide's stage down (never up) to fit the viewport. Below a `0.6` floor the text stops being projectable, so the slide instead keeps the floor scale, anchors to the top, and **scrolls vertically** — that is the escape hatch for one tall artifact (a code walkthrough, a long diagram), not licence to cram a section.

### Composing the slide manifest

The deck is declared as `initPresent([...])` at the end of the page script — one object per slide. **Never map sections to slides 1:1.** Recompose:

- A short section (heading + one artifact) → adopt the whole `<section>`; the editorial two-column layout and section number carry over.
- A long section → split it across slides. Add `id`s at the split points in the HTML, give later slides a synthesized `kicker` (e.g. `'04 · Preparing for Cedar'`) and optional `title` so the chapter context survives the split.
- An interactive widget → its own slide, with a `steps` driver so arrow keys play it.
- A widget plus its full section heading is usually too tall — adopt only the widget (e.g. `'#walkthrough .stepper'`) and synthesize the title.
- Budget each slide to render at scale ≥ 0.75 on a 1600×900 window. Widgets that measure pixels at runtime (e.g. a needle positioned via `getBoundingClientRect`) misalign under scaling — keep those slides light enough to render at scale 1.
- **Stepper steps set the scale budget, not the section.** When `clickSteps` drives a stepper, `fit()` measures the stage *after each step is rendered*. If one step is too tall — a common case is a diagram plus a long code block plus prose all in one step — the slide scales below the floor and becomes unreadable. The fix is to split that step in the `steps` data array (add a new step object containing only the overflow content). You cannot target an individual step state from `pick`, so splitting the manifest entry does not help; the split must happen in the data.
- **Rail-tab steps with varying content heights will rescale the slide on every click.** When a `clickSteps`-driven slide renders different amounts of content per tab (e.g. 2 rows vs 9 rows), each step change triggers `refit()` with a new `offsetHeight`, causing the whole slide to rescale and font size to visibly shift. The fix is to pre-measure the worst-case height across all steps on first slide entry and cache it on the slide object (`s._maxH`), so `fit()` always uses the same reference height regardless of which step is currently showing. The resize handler should clear `_maxH` so it remeasures for the new viewport. Additionally: (1) suppress `itemSlideIn` row animations inside `.pr-stage` (`.pr-stage .afile { animation: none !important }`) so tab switches are instant; (2) do not call `refit()` in `next()`/`prev()` for `_maxH`-stabilised slides; (3) if the content area (e.g. a file-list grid) changes height between tabs, pin it with `.pr-stage .my-container { min-height: Xpx; align-content: start }` so the stage height — and therefore vertical centring — stays constant.
- **Steppers with genuinely variable-height steps** (prose + code that grows and shrinks per step rather than a uniform row list) need a different strategy. Adding `minScale: 0.85` to the slide object tells `fit()` to floor the scale at 0.85 for that slide specifically — font size never changes between steps, and when a step is too tall the whole slide scrolls (`pr-scroll`) at that scale instead of shrinking the text. `refit()` is called on each step advance for `minScale` slides so `marginBottom` stays correct as content height changes. Slides without `minScale` continue to use `_maxH` with the global `MIN_SCALE` floor. Apply `minScale: 0.85` to any stepper slide where step content height varies significantly (e.g. walkthrough and binaries steppers).

Slide object fields:

| Field | Meaning |
|---|---|
| `pick` | Array of selectors; each first match is adopted into the slide, in order. |
| `kicker` / `title` | Synthesized header for slides cut out of a larger section. |
| `layout` | `'wide'` (1320px stage — screenshots), `'diagram'` (`.arch-svg` fills the stage). Default stage is 1100px. |
| `steps` | `{ count, go(i) }` fragment driver. `go(0)` is the slide's entry state; entering backward applies `go(count - 1)`. |
| `minScale` | Number (e.g. `0.85`). Floors the scale at this value for this slide; the slide scrolls rather than shrinking text. Use for steppers with variable-height step content. |

Three ways to drive fragments:

```js
// 1. Walk existing tabs/dots — one press per button
{ pick: ['#shape'], steps: clickSteps('#rail .rail-btn') }

// 2. Reveal items one by one (adds .pr-frag / .pr-on)
{ pick: ['#problem'], steps: fragSteps('#problem .mini-list > li') }

// 3. Call page functions directly (same script scope)
{ pick: ['#heroDiagram'], layout: 'diagram',
  steps: { count: 8, go: (i) => showSeq(i - 1) } } // step 0 = idle state
```

### Markup

Add the toggle right after the theme toggle:

```html
<button class="present-toggle" id="presentToggle" type="button" aria-label="Present (P)" title="Present (P)">▶</button>
```

### CSS (paste as-is)

```css
/* ── Present mode ── */
.present-toggle{position:fixed;top:24px;right:80px;z-index:100;width:44px;height:44px;border:1px solid var(--border);border-radius:50%;background:var(--surface);color:var(--text-dim);cursor:pointer;display:grid;place-items:center;transition:all .4s var(--ease-out-expo);font-size:14px}
.present-toggle:hover{border-color:var(--accent);color:var(--accent);transform:scale(1.1)}
body.presenting{overflow:hidden}
body.presenting .theme-toggle,body.presenting .present-toggle{z-index:300}
.pr-deck{position:fixed;inset:0;z-index:200;background:var(--bg);display:none;outline:none}
.pr-deck::before{content:'';position:absolute;top:-220px;right:-220px;width:600px;height:600px;border-radius:50%;background:radial-gradient(circle,var(--accent-dim),transparent 70%);opacity:.25;pointer-events:none}
body.presenting .pr-deck{display:block}
.pr-slide{position:absolute;inset:0;display:flex;align-items:center;justify-content:center;padding:56px 48px 96px;opacity:0;visibility:hidden;transition:opacity .45s var(--ease-out-expo),visibility 0s linear .45s}
.pr-slide.active{opacity:1;visibility:visible;transition:opacity .45s var(--ease-out-expo) .05s,visibility 0s}
.pr-stage{width:min(1100px,94vw);transform-origin:center center}
.pr-slide.pr-scroll{align-items:flex-start;overflow-y:auto}
.pr-slide[data-layout="wide"] .pr-stage{width:min(1320px,96vw)}
.pr-slide[data-layout="diagram"] .arch-svg{max-width:100%}
.pr-stage>*+*{margin-top:28px!important}
.pr-stage>:first-child{margin-top:0!important}
.pr-stage>.pr-title{margin-top:14px!important}
.pr-stage section{padding:0!important}
.pr-stage section::before{display:none!important}
.pr-stage .hero{min-height:0!important;padding-bottom:0!important}
.pr-kicker{font-family:var(--mono);font-size:.7rem;letter-spacing:.18em;text-transform:uppercase;color:var(--text-dim);display:flex;align-items:center;gap:12px}
.pr-kicker::before{content:'';width:40px;height:1px;background:var(--accent)}
.pr-title{font-size:clamp(1.6rem,3vw,2.6rem);font-weight:700;letter-spacing:-0.02em}
.pr-hud{position:fixed;left:50%;bottom:28px;transform:translateX(-50%);z-index:300;display:none;align-items:center;gap:4px;padding:6px;border:1px solid var(--border);border-radius:100px;background:color-mix(in srgb,var(--surface) 88%,transparent);backdrop-filter:blur(12px);transition:opacity .4s ease}
body.presenting .pr-hud{display:flex}
.pr-hud.idle{opacity:0;pointer-events:none}
.pr-hud button{appearance:none;border:0;background:transparent;color:var(--text-dim);width:36px;height:36px;border-radius:50%;cursor:pointer;font-size:.95rem;display:grid;place-items:center;transition:all .3s var(--ease-out-expo)}
.pr-hud button:hover:not(:disabled){background:var(--accent-dim);color:var(--accent)}
.pr-hud button:disabled{opacity:.3;cursor:default}
.pr-count{font-family:var(--mono);font-size:.72rem;color:var(--text-dim);padding:0 10px;min-width:64px;text-align:center}
.pr-progress{position:fixed;left:0;bottom:0;height:2px;width:100%;z-index:300;display:none;background:color-mix(in srgb,var(--border) 60%,transparent)}
body.presenting .pr-progress{display:block}
.pr-progress i{display:block;height:100%;width:0;background:linear-gradient(90deg,var(--accent-dim),var(--accent));transition:width .45s var(--ease-out-expo)}
.pr-deck .pr-frag{opacity:0;transform:translateY(14px);transition:opacity .5s var(--ease-out-expo),transform .5s var(--ease-out-expo)}
.pr-deck .pr-frag.pr-on{opacity:1;transform:none}
@media(max-width:900px){.present-toggle{display:none}}
/* Restore desktop layouts inside the deck at any browser zoom level.
   CMD+/CMD- shrinks the CSS viewport and triggers responsive breakpoints,
   collapsing grids to single-column. These overrides defeat that. */
.pr-stage .hero-grid{grid-template-columns:1fr minmax(300px,0.46fr)}
.pr-stage .cols{grid-template-columns:minmax(200px,0.38fr) 1fr}
.pr-stage .compare-grid,.pr-stage .pair-grid,.pr-stage .gv-grid{grid-template-columns:1fr 1fr}
.pr-stage .adapter-flow{grid-template-columns:repeat(3,1fr)}
.pr-stage .adapter-step::after{display:block}
.pr-stage .afile{grid-template-columns:minmax(140px,0.32fr) 1fr!important;gap:20px!important}
.pr-stage .fact{grid-template-columns:140px 1fr;gap:16px}
.pr-stage .bd{grid-template-columns:1fr 168px 1fr}
.pr-stage .bd-seam{flex-direction:column;flex-wrap:nowrap;padding:18px 8px}
.pr-stage .bd-seam::before{top:18px;bottom:18px;left:50%;right:auto;border-left:1px dashed var(--border-strong);border-top:0}
.pr-stage .arch{grid-template-columns:1fr auto 1fr;gap:0}
.pr-stage .arch-link{flex-direction:column;padding:0 18px}
.pr-stage .mm{flex-direction:row}
.pr-stage .mm-arrow{transform:none}
.pr-stage .live-subject{flex-basis:500px}
.pr-stage .live-arrow{width:62px}
```

### JS kit (paste at the end of the page script, then call `initPresent`)

```js
/* ── Present mode ── */
function fragSteps(selector){
  var items = Array.prototype.slice.call(document.querySelectorAll(selector));
  items.forEach(function(el){ el.classList.add('pr-frag'); });
  return {
    count: items.length + 1,
    go: function(i){ items.forEach(function(el, k){ el.classList.toggle('pr-on', k < i); }); }
  };
}
function clickSteps(selector){
  var count = document.querySelectorAll(selector).length;
  return {
    count: count,
    go: function(i){ var b = document.querySelectorAll(selector)[i]; if(b) b.click(); }
  };
}
function initPresent(slides){
  var btn = document.getElementById('presentToggle');
  if(!btn) return;
  var deck = null, hud, countEl, barEl, prevBtn, nextBtn;
  var adopted = [], cur = 0, step = 0, savedScroll = 0, hudTimer = null;

  function stepCount(s){ return s.steps ? s.steps.count : 1; }
  function presenting(){ return document.body.classList.contains('presenting'); }

  function build(){
    deck = document.createElement('div');
    deck.className = 'pr-deck'; deck.tabIndex = -1;
    deck.setAttribute('role', 'region'); deck.setAttribute('aria-label', 'Presentation');
    slides.forEach(function(s){
      var sl = document.createElement('section');
      sl.className = 'pr-slide';
      if(s.layout) sl.dataset.layout = s.layout;
      var stage = document.createElement('div');
      stage.className = 'pr-stage';
      if(s.kicker){ var k = document.createElement('div'); k.className = 'pr-kicker'; k.textContent = s.kicker; stage.appendChild(k); }
      if(s.title){ var t = document.createElement('h2'); t.className = 'pr-title'; t.textContent = s.title; stage.appendChild(t); }
      sl.appendChild(stage);
      deck.appendChild(sl);
      s._slide = sl; s._stage = stage;
    });
    hud = document.createElement('div'); hud.className = 'pr-hud';
    prevBtn = hudButton('←', 'Previous'); nextBtn = hudButton('→', 'Next');
    countEl = document.createElement('span'); countEl.className = 'pr-count';
    hud.appendChild(prevBtn); hud.appendChild(countEl); hud.appendChild(nextBtn);
    barEl = document.createElement('div'); barEl.className = 'pr-progress';
    barEl.appendChild(document.createElement('i'));
    document.body.appendChild(deck);
    document.body.appendChild(hud);
    document.body.appendChild(barEl);
    prevBtn.addEventListener('click', prev);
    nextBtn.addEventListener('click', next);
  }
  function hudButton(label, name){
    var b = document.createElement('button');
    b.type = 'button'; b.textContent = label; b.setAttribute('aria-label', name);
    return b;
  }

  function adopt(){
    slides.forEach(function(s){
      (s.pick || []).forEach(function(sel){
        var node = document.querySelector(sel);
        if(!node) return;
        adopted.push({ el: node, parent: node.parentNode, next: node.nextSibling });
        s._stage.appendChild(node);
        if(node.classList.contains('reveal')) node.classList.add('in', 'is-visible');
        node.querySelectorAll('.reveal').forEach(function(r){ r.classList.add('in', 'is-visible'); });
      });
    });
  }
  function restore(){
    for(var i = adopted.length - 1; i >= 0; i--){
      adopted[i].parent.insertBefore(adopted[i].el, adopted[i].next);
    }
    adopted = [];
  }

  // Scale the stage down (never up) so adopted content always fits the viewport.
  // Below MIN_SCALE text stops being readable from the back of a room, so
  // tall content keeps the floor scale and the slide scrolls vertically instead.
  var MIN_SCALE = 0.6;
  function fit(s){
    var availW = s._slide.clientWidth - 96;
    var availH = s._slide.clientHeight - 152;
    var scale = Math.min(1, availW / s._stage.offsetWidth, availH / s._stage.offsetHeight);
    var scrolls = scale < MIN_SCALE;
    if(scrolls) scale = MIN_SCALE;
    s._slide.classList.toggle('pr-scroll', scrolls);
    s._stage.style.transformOrigin = scrolls ? 'top center' : 'center center';
    // transform does not shrink the layout box — collapse the leftover so
    // the scroll height matches what is visually there.
    s._stage.style.marginBottom = scrolls ? (-(1 - scale) * s._stage.offsetHeight) + 'px' : '';
    s._stage.style.transform = scale < 1 ? 'scale(' + scale + ')' : 'none';
  }
  function refit(){
    var s = slides[cur];
    requestAnimationFrame(function(){ fit(s); s._slide.scrollTop = 0; });
  }

  function syncHud(){
    var s = slides[cur];
    countEl.textContent = (cur + 1) + ' / ' + slides.length;
    barEl.firstChild.style.width = (((cur + 1) / slides.length) * 100) + '%';
    prevBtn.disabled = cur === 0 && step === 0;
    nextBtn.disabled = cur === slides.length - 1 && step === stepCount(s) - 1;
  }
  function show(i, dir){
    cur = i;
    var s = slides[i];
    slides.forEach(function(o, j){
      o._slide.classList.toggle('active', j === i);
      o._slide.setAttribute('aria-hidden', j === i ? 'false' : 'true');
    });
    step = dir < 0 ? stepCount(s) - 1 : 0;
    if(s.steps) s.steps.go(step);
    syncHud();
    refit();
  }
  function next(){
    var s = slides[cur];
    if(s.steps && step < s.steps.count - 1){ step++; s.steps.go(step); syncHud(); refit(); }
    else if(cur < slides.length - 1){ show(cur + 1, 1); }
  }
  function prev(){
    var s = slides[cur];
    if(s.steps && step > 0){ step--; s.steps.go(step); syncHud(); refit(); }
    else if(cur > 0){ show(cur - 1, -1); }
  }

  function enter(){
    if(!deck) build();
    savedScroll = window.scrollY;
    adopt();
    document.body.classList.add('presenting');
    btn.textContent = '✕'; btn.setAttribute('aria-label', 'Exit presentation (Esc)');
    show(0, 1);
    deck.focus();
    wakeHud();
  }
  function exit(){
    document.body.classList.remove('presenting');
    btn.textContent = '▶'; btn.setAttribute('aria-label', 'Present (P)');
    slides.forEach(function(s){ s._slide.classList.remove('active'); });
    restore();
    window.scrollTo(0, savedScroll);
  }
  function toggle(){ if(presenting()){ exit(); } else { enter(); } }

  function wakeHud(){
    if(!hud) return;
    hud.classList.remove('idle');
    clearTimeout(hudTimer);
    hudTimer = setTimeout(function(){ hud.classList.add('idle'); }, 3000);
  }

  btn.addEventListener('click', toggle);
  window.addEventListener('resize', function(){ if(presenting()) refit(); });
  window.addEventListener('mousemove', function(){ if(presenting()) wakeHud(); });
  window.addEventListener('keydown', function(e){
    var t = e.target;
    if(t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return;
    if(!presenting()){
      if(e.key === 'p' || e.key === 'P'){ toggle(); e.preventDefault(); }
      return;
    }
    switch(e.key){
      case 'ArrowRight': case 'ArrowDown': case ' ': case 'PageDown': next(); e.preventDefault(); break;
      case 'ArrowLeft': case 'ArrowUp': case 'PageUp': prev(); e.preventDefault(); break;
      case 'Home': show(0, 1); e.preventDefault(); break;
      case 'End': show(slides.length - 1, 1); e.preventDefault(); break;
      case 'Escape': exit(); break;
      case 'f': case 'F':
        if(document.fullscreenElement){ document.exitFullscreen(); }
        else if(document.documentElement.requestFullscreen){ document.documentElement.requestFullscreen(); }
        break;
      case 'p': case 'P': toggle(); break;
    }
  });
}
```

### Worked manifest (from the services-internal explainer)

```js
// Slide manifest — recomposes the page; never a blind section-per-slide map.
initPresent([
  { pick: ['#top'] },                                                  // hero = title slide
  { pick: ['#problem'], steps: fragSteps('#problem .mini-list > li') },// reveal pains one by one
  { pick: ['#rule'], steps: fragSteps('#rule tbody tr') },             // table rows as fragments
  { pick: ['#shape'], steps: clickSteps('#rail .rail-btn') },          // arrows walk the rail tabs
  { kicker: '04 · Preparing for Cedar', pick: ['#boundaryHead', '#boundaryLede', '#bdDiagram'] },
  { kicker: '04 · Preparing for Cedar', title: 'The boundary, in code', pick: ['#authzCode'] },
  { kicker: '04 · Preparing for Cedar', pick: ['#archLead', '#archDiagram', '#cedarCode'] },
  { kicker: '05 · Walkthrough', title: 'Adding an entity, step by step',
    pick: ['#walkthrough .stepper'], steps: clickSteps('#entDots .step-dot') },
  // …
]);
```

### Cursor timeline (incremental pull visualization)

For topics that involve incremental sync or stream processing, a horizontal timeline with a draggable needle works well.

**CSS:**

```css
.cursor-stage { border: 1px solid var(--border); border-radius: 16px;
  padding: 32px; background: var(--surface); min-height: 320px; }
.cursor-timeline { display: flex; align-items: center; justify-content: space-between;
  margin: 36px 8px 40px; font-family: var(--mono); font-size: 0.72rem; position: relative; }
.cursor-track { position: absolute; left: 0; right: 0; top: 5px; height: 2px;
  background: var(--border); z-index: 0; }
.cursor-track-pulled { position: absolute; left: 0; top: 5px; height: 2px;
  background: var(--success); z-index: 0; transition: width 0.5s var(--ease-out-expo); width: 0%; }
.cursor-station { display: flex; flex-direction: column; align-items: center; gap: 10px;
  position: relative; z-index: 1; flex-shrink: 0; }
.cursor-station-dot { width: 12px; height: 12px; border-radius: 50%;
  background: var(--border-strong); transition: all 0.5s var(--ease-out-expo); position: relative; }
.cursor-station-dot.pulled { background: var(--success); }
.cursor-station-dot.active { background: var(--accent); box-shadow: 0 0 0 5px var(--accent-dim); }
.cursor-needle { position: absolute; top: -10px; width: 28px; height: 28px; border-radius: 50%;
  background: var(--accent); color: var(--bg); display: grid; place-items: center;
  font-size: 0.65rem; font-weight: 800; box-shadow: 0 0 0 6px var(--accent-dim), 0 4px 16px rgba(0,217,255,0.25);
  transition: left 0.6s var(--ease-out-expo), opacity 0.3s ease; opacity: 0; z-index: 2; pointer-events: none; }
.cursor-needle.visible { opacity: 1; }
.cursor-legend { display: flex; gap: 20px; font-family: var(--mono);
  font-size: 0.7rem; color: var(--text-dim); }
.cursor-legend span { display: inline-flex; align-items: center; gap: 6px; }
.cursor-legend i { display: inline-block; width: 8px; height: 8px; border-radius: 50%; }
.cursor-panel { margin-top: 28px; min-height: 80px; }
.cursor-panel-line { font-family: var(--mono); font-size: 0.82rem; line-height: 1.7;
  color: var(--text-dim); transition: opacity 0.3s ease, transform 0.3s ease; }
.cursor-panel-line.new { animation: cursorPanelIn 0.4s var(--ease-out-expo) forwards; }
@keyframes cursorPanelIn {
  from { opacity: 0; transform: translateY(8px); }
  to   { opacity: 1; transform: translateY(0); }
}
```

### HLC conflict resolution demo

For distributed systems topics, a live walkthrough of actors writing conflicting values is powerful.

**CSS:**

```css
.hlc-stage { border: 1px solid var(--border); border-radius: 16px; padding: 32px; background: var(--surface); }
.hlc-actors { display: grid; grid-template-columns: 1fr 1fr; gap: 16px; margin-bottom: 20px; }
.hlc-actor { border: 1px solid var(--border); border-radius: 12px; padding: 20px;
  background: var(--bg); transition: border-color 0.5s var(--ease-out-expo), background 0.5s var(--ease-out-expo), opacity 0.5s ease; }
.hlc-actor.hlc-stored { border-color: color-mix(in srgb, var(--accent) 50%, var(--border)); background: var(--accent-dim); }
.hlc-actor.hlc-winner { border-color: color-mix(in srgb, var(--success) 50%, var(--border)); background: var(--success-dim); }
.hlc-actor.hlc-loser { border-color: color-mix(in srgb, var(--error) 40%, var(--border)); background: var(--error-dim); opacity: 0.55; }
.hlc-actor-name { font-family: var(--mono); font-size: 0.68rem; text-transform: uppercase;
  letter-spacing: 0.1em; color: var(--text-dim); margin-bottom: 10px; }
.hlc-actor-value { font-size: 1.1rem; font-weight: 700; color: var(--text); margin-bottom: 10px; }
.hlc-ts { font-family: var(--mono); font-size: 0.72rem; padding: 8px 12px;
  background: var(--surface); border-radius: 8px; border: 1px solid var(--border); line-height: 1.7; }
.hlc-ts-wall    { color: var(--accent); }
.hlc-ts-counter { color: var(--warning); }
.hlc-ts-actor   { color: var(--success); }
.hlc-server { border: 1px solid var(--border); border-radius: 12px; padding: 20px 24px;
  background: var(--bg); min-height: 100px; font-family: var(--mono);
  font-size: 0.78rem; line-height: 1.8; color: var(--text-dim); }
.hlc-log-line { animation: hlcLogIn 0.3s var(--ease-out-expo); }
@keyframes hlcLogIn {
  from { opacity: 0; transform: translateX(-8px); }
  to   { opacity: 1; transform: translateX(0); }
}
.hlc-verdict { margin-top: 14px; padding: 12px 18px; border-radius: 10px;
  font-family: var(--mono); font-size: 0.78rem; opacity: 0;
  transition: opacity 0.5s ease, transform 0.5s var(--ease-out-expo); transform: translateY(6px); }
.hlc-verdict.visible { opacity: 1; transform: translateY(0); }
.hlc-verdict.good { background: var(--success-dim); border: 1px solid color-mix(in srgb, var(--success) 40%, var(--border)); color: var(--success); }
.hlc-verdict.info { background: var(--accent-dim); border: 1px solid color-mix(in srgb, var(--accent) 40%, var(--border)); color: var(--accent); }
```

### Growth-phase tabs (dynamic payload panels)

For content that has multiple phases or versions, use a `.rail` of buttons that swap both prose and code panels.

**Pattern:**

- Render the rail buttons into `#growthRail`. (`.rail`/`.rail-btn` styles live in `services-internal-architecture-explainer.html` — boxed *controls* are exempt machinery.)
- Maintain a `phases[]` array with `{label, sublabel, proseHTML, code}`.
- On click, replace `#growthCopy` and `#growthPayload` content, then run `highlightAll()` on any new code blocks.
- Use `detail-grid` with `grid-template-columns: 0.8fr 1.2fr` when prose is left and code is right.

## Implementation constraints

- Single `.html` file, inline CSS and JS, no external runtime dependencies.
- JavaScript must be small and readable.
- Works offline and is self-contained (images as base64, no CDN fonts — use system stack).
- Semantic HTML where practical.
- No secrets, real customer data, or internal tokens. Synthetic example data only.

## Verification

After writing the HTML:

1. **Banned-vocabulary sweep.** Grep the file for emoji outside the approved glyphs (◐ ▶ ✕ ✓ — · ↑ → ↗ arrows), and for `pill|badge|chip|card` in class names, `<table`, and `prog-bar`. Any hit is a design violation — fix it before anything else.
2. Validate JS: extract the `<script>` body and run `node -e "new Function(require('fs').readFileSync('/tmp/s.js','utf8'))"` or equivalent.
3. **Overflow sweep at 1440 / 820 / 390.** With Playwright, after forcing `.reveal` visible, walk `body *` and flag any element where `scrollWidth - clientWidth > 4` and `overflow-x` is not auto/scroll/hidden, plus page-level `documentElement.scrollWidth > clientWidth`. Text bleeding outside its box is the single most common defect — never ship without this sweep. (Expected exception: `::after` flow arrows positioned in a grid gap.)
4. **Click every interactive state once**: each filter tab/rail button (assert it shows > 0 items — a `data-cat` value that doesn't match the filter argument shows an empty grid and no error), each popover via real click *and* keyboard focus, both themes. While there, grep the file for `var(--` names not defined in `:root` — an undefined custom property fails silently.
5. Walk Present mode end to end (Playwright/chromium from the repo, or system Chrome): enter with the `▶` button, arrow through every slide and fragment to the end, screenshot along the way, then `Esc` and confirm the page is restored (including any filter state a tab-stepping slide changed). Watch for slides scaled below ~0.75 (split them), pixel-measuring widgets under scale (lighten the slide), and console errors. Note: `scroll-behavior:smooth` makes `scrollIntoView` async — use `behavior:'instant'` in test scripts before sampling coordinates, or synthetic clicks land on the wrong element.
6. Confirm `git status --porcelain` so the user can see whether the file is tracked or untracked.

## Final response

Keep it short:

- File path created/updated.
- Main interactive pieces included.
- Slide count and how Present mode splits the sections.
- Whether screenshots were embedded.
- JS validation + Present walkthrough result.
- Tracked or untracked.
