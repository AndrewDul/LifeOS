# R001 — M1 UI Refinement: Home Edge Drawers, Face Geometry, Constellation Background, Chat Sidebar

Date: 2026-09-17
Scope: M1 visual refinement only. No M2 (backend/database/AI) work.

This is a one-off report for this specific change, requested explicitly
by the user for this task. Standing project policy (`CLAUDE.md`,
"Documentation policy") is normally to avoid one-off dated report files
in favor of the four living docs plus git history — that policy still
governs by default; this file is a deliberate, explicitly-requested
exception for this pass, not a change to the policy itself. See the
note at the end of this report and this run's chat reply for the
flagged tension.

---

## 1. What was changed

1. **Home page: slide-out edge drawers restored**, redesigned to be
   hidden by default with subtle, minimal edge handles rather than the
   permanently-visible prototype tabs from an earlier pass:
   - **Top:** Notifications, Settings
   - **Left:** Dashboard (→ the `today` screen), Goals
   - **Right:** Projects, Me / Insights (→ `me` and `insights`)
   - **Bottom:** Chat
   Each edge shows one or two small tab-style handles; clicking one
   slides in a glass panel from that edge with a short blurb and, where
   a full screen already exists, an "Open X" action. Only one edge panel
   is open at a time; clicking the active handle again, clicking a
   scrim, or navigating away closes it cleanly back to Home.

2. **NeXa face geometry refined:** eyes made more elongated (26×52,
   rx 13, was 30×42, rx 15) and the mouth pushed further down relative to
   the eyes (arc/ellipse shifted from y≈158-186 to y≈166-192) while
   keeping the same simple two-eyes-and-a-smile identity.

3. **Background/constellation strengthened:**
   - Star count roughly doubled (8 clusters × 20-36 stars + 100 sparse
     background stars, was 6 clusters × 14-24 + 60).
   - Drift animation made clearly perceptible: star-field drift period
     cut from 180s→100s and amplitude from ~1vmin→~2.2-2.7vmin; haze
     drift similarly strengthened (220-260s→130-160s, larger translate).
     Still slow and calm, not fast or distracting.
   - Added one deliberately larger, brighter **constellation formation**:
     6-7 connected points in the upper-right area, rendered as brighter
     stars with a very faint (`rgba(230,235,255,0.16)`, 0.12 stroke
     width, non-scaling) connecting polyline - a single visible anchor
     shape rather than a field of uniform clusters.
   - Background stayed almost-black; the added density is mostly very
     low-opacity sparse stars so it doesn't wash out the dark base.

4. **Chat screen: dedicated chat sidebar added**, separate from the
   global section navigation:
   - New `ChatSidebar` component: "+ New Chat" button, a (visually
     present, non-functional) search input, and a scrollable list of
     demo chats (title + preview), with an active-selection state.
   - `ChatScreen` now renders `[global SideNavigation] [ChatSidebar]
     [chat-main]` inside the existing `ScreenShell` - the global nav
     rail is unchanged and still present; the new sidebar is visually
     distinct (a rounded glass panel, not a rail) and serves a different
     purpose (which conversation, not which app section).
   - Selecting a chat swaps in that chat's own small demo transcript
     (3 demo conversations); "New Chat" shows an empty state ("Start a
     new conversation") instead of pretending to create persisted
     history that doesn't exist.

## 2. Why it was changed

Direct response to explicit user feedback on the previous visual pass:
edge drawers were wanted back (differently executed - hidden/subtle,
not permanent prototype tabs); the face needed more elongated eyes and
a lower mouth; the background felt too static and the "constellation"
language wasn't literally present; and the Chat screen's sidebar read
as a duplicate of the global app menu rather than a chat-specific
history panel, which the user explicitly said was wrong.

## 3. Files modified

**New:**
- `src/components/ChatSidebar.tsx`
- `docs/reports/R001_M1_UI_REFINEMENT_HOME_DRAWERS_AND_CHAT_SIDEBAR_2026-09-17.md` (this file)

**Recreated** (previously deleted in the last visual-refinement pass,
now rebuilt with a different, more minimal design):
- `src/components/EdgeDrawers.tsx`

**Modified:**
- `src/components/NexaFace.tsx` (eye/mouth geometry)
- `src/components/Galaxy.tsx` (star count, drift, constellation)
- `src/components/ChatScreen.tsx` (sidebar integration, per-chat demo
  transcripts, new-chat empty state)
- `src/App.tsx` (mount `EdgeDrawers` on Home)
- `src/App.css` (edge-drawer styles, strengthened galaxy drift/
  constellation styles, chat-sidebar + chat-main/chat-empty styles)
- `docs/PROJECT_STATE.md` (current state updated)

## 4. Architecture / component decisions

- **EdgeDrawers is one component, not four.** All four edges share a
  declarative config (`edge`, list of `{id, label, blurb, actions?}`
  tabs) and one render pass, rather than four near-duplicate components -
  keeps behavior (open/close/scrim/tab-switch) in one place and easy to
  extend if a fifth edge concept is ever needed.
- **Drawers are previews, not duplicate screens.** Each tab shows a
  short blurb and, where relevant, an "Open X" button that calls the
  same `onNavigate`/`ScreenId` mechanism the radial menu and sidebar
  already use - no separate navigation system, no duplicated screen
  logic.
- **"Notifications" has no full screen and no action button.**
  `LIFEOS_SPEC.md` treats notifications as a drawer-only concept (M7),
  not a primary screen, so its drawer content is a plain placeholder
  line rather than a fabricated "Open Notifications" link to a screen
  that doesn't exist.
- **"Dashboard" is a drawer-only label for the `today` screen.** The
  global navigation (radial menu, side nav) still calls it "Today"
  everywhere else, per the instruction to keep "the global screen
  navigation structure ... separate" and unchanged. This is a narrow,
  intentional naming inconsistency scoped to one drawer handle - flagged
  here rather than silently introduced.
- **"Me / Insights" is one tab with two action buttons**, not two
  separate tabs, matching how the brief listed it as a single bullet.
- **Chat's global SideNavigation was kept, not removed.** The brief said
  the chat-specific sidebar "is NOT... the global application
  navigation" and that "the global screen navigation structure must
  remain separate" - read as "keep both, make them visually distinct,"
  not "replace the global rail with the chat sidebar." `ScreenShell`
  (shared by every full-screen section) was left untouched; `ChatScreen`
  adds its own sidebar inside its content area alongside it.
- **Chat's per-chat "history" is static demo data**, not fake
  client-side persistence - clicking "New Chat" does not add a fake
  saved entry to the list (which would misrepresent state that doesn't
  exist); it shows an explicit empty state instead.

## 5. Tradeoffs

- The constellation shape and star cluster positions are randomized per
  page load (no fixed seed) - the star field will look slightly
  different on every launch. This matches the existing (pre-refinement)
  Galaxy approach and keeps the code simple; a fixed/seeded layout would
  need a small PRNG utility if exact reproducibility is ever wanted.
- Drift amplitude/speed values (100-160s periods, 2-4vmin translation)
  are a judgment call between "does not move enough" (the complaint) and
  "not distracting" (the constraint) - genuinely a matter of visual
  taste that only a live look can confirm.
- The edge-drawer handle labels use `writing-mode: vertical-rl` for the
  left/right edges (matching the previous pass's approach) rather than
  horizontal text in a wider hit area - keeps the handles small and
  unobtrusive per "elegant and minimal," at a minor cost to at-a-glance
  readability until hovered/focused.

## 6. Validation steps and results

| Check | Command | Result |
|---|---|---|
| Frontend build | `npm run build` (`tsc && vite build`) | **PASS** - 28 modules transformed, built in 142ms, zero TypeScript errors |
| Rust check | `cargo check --manifest-path src-tauri/Cargo.toml` | **PASS** - full dependency graph compiled and linked in 9.30s, zero errors |

No LifeOS-authored code caused any build failure. **No visual PASS is
claimed** - per standing project policy, only compile/type-check
evidence exists from this session; a human needs to run `npm run tauri
dev` and look at the result.

## 7. Anything still pending

- **Visual re-verification by the user** of every change in this report
  (drawers, face geometry, background motion/constellation, chat
  sidebar) - nothing here has been seen running.
- The documentation-policy tension noted at the top of this file (one-
  off report files vs. `CLAUDE.md`'s stated policy) is unresolved -
  flagged in this run's chat reply for the user to decide whether to
  amend `CLAUDE.md` for future UI-refinement passes or treat this as a
  one-time exception.
- Everything already listed as pending in `docs/PROJECT_STATE.md` §8
  (local/cloud model integration, data-sensitivity classification, Goal
  Intelligence, Personal Knowledge, NeXa capability API, bilingual
  mechanism, LifeOS's own license) is unchanged by this visual-only
  pass.
