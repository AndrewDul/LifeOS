# R002 — M1 Navigation Correction: Full-Screen Edge Transitions, Global Sidebar Removed, NeXa Island as Sole Global Nav

Date: 2026-09-17
Scope: M1 visual/interaction correction only. No M2 (backend/database/AI) work.

Per the standing note in the previous report, this remains a deliberate
exception to `CLAUDE.md`'s "no one-off dated report files" policy,
explicitly requested again for this pass.

---

## 1. What changed

This corrects the navigation model from the two prior visual-refinement
passes, which had drifted from what was actually wanted:

1. **Edge handles now open the real screen directly** - clicking a Home
   edge handle no longer opens a small preview panel with a blurb and an
   "Open X" button. It opens the actual full-screen section immediately,
   sliding in from that edge and filling the viewport.
2. **The global navigation sidebar is gone entirely.** No screen -
   Chat, Today, Calendar, Goals, Projects, Money, Me, Insights, Settings,
   Notifications - shows a persistent left-hand app-section rail anymore.
3. **The NeXa Island is now the sole persistent global navigation
   element** while any section is open. Its quick menu (Home / Menu) is
   the only way to close a section or jump to another one without going
   through an edge handle or the radial menu.
4. **Chat keeps its own dedicated sidebar** (New Chat / search / recent
   chats, from the previous pass) - that one is Chat's own feature, not
   global navigation, and was explicitly preserved.
5. **Home is unchanged in kind** (large face, radial menu, edge handles,
   galaxy) and still does not use the NeXa Island - the large face
   remains Home's own navigation center.

## 2. Why it changed

Direct response to explicit feedback: the previous edge-drawer preview
panels were "too small" and needed to become actual full application
windows; the global sidebar was still visible inside every open section
and needed to disappear completely, leaving the NeXa Island as the only
persistent global navigation surface; and Chat's own history sidebar
needed to be confirmed as staying, distinct from that removed global
rail.

## 3. Files changed

**Deleted:**
- `src/components/SideNavigation.tsx` - the global nav rail no longer
  exists anywhere, so the component was removed rather than left unused.

**Rewritten:**
- `src/components/EdgeDrawers.tsx` - no more internal open/closed/tab
  state, no panel rendering. Now a small stateless component: four edge
  groups of handle buttons, each calling `onOpen(screenId, direction)`.
- `src/components/ScreenShell.tsx` - no longer renders `SideNavigation`.
  Now takes `direction`/`closing` props and applies them as
  `data-direction`/`data-closing` attributes that drive the CSS slide
  animation. Still renders `NexaIsland` + a content area.
- `src/App.tsx` - added the direction/closing navigation state (see §4)
  and `openScreen`/`closeScreen` functions; `EdgeDrawers`, `RadialMenu`,
  and `NexaIsland`'s navigation now all funnel through `openScreen` with
  an appropriate `Direction`; the NeXa Island's Home action now calls
  `closeScreen` (animated) instead of setting state directly.

**Modified:**
- `src/state/navigation.ts` - added a `Direction` type
  (`"top"|"bottom"|"left"|"right"|"radial"`); added a `notifications`
  `ScreenId` (see §6) so the Notifications edge handle has a real target;
  renamed the former `SIDEBAR_SCREENS` export to `ALL_SCREENS` since it
  no longer feeds a sidebar - it now only feeds the NeXa Island's quick
  navigation list.
- `src/components/NexaIsland.tsx` - updated the import to `ALL_SCREENS`;
  updated its doc comment to state plainly that it is now the *only*
  persistent global navigation element.
- `src/components/PlaceholderScreen.tsx`, `src/components/ChatScreen.tsx`
  - both now accept and forward `direction`/`closing` props to
  `ScreenShell`; `PlaceholderScreen`'s milestone map gained a
  `notifications: "M7"` entry.
- `src/App.css` - removed all `.side-nav*` rules and all `.edge-panel*`/
  `.edge-panel-tab*`/`.edge-scrim` rules (both concepts no longer exist);
  rewrote the `.screen-shell` section with the new directional
  enter/close animations (see §5); widened `.screen-shell-content` now
  that no sidebar column takes up space.
- `docs/PROJECT_STATE.md` - current state updated (see that file).

## 4. Navigation architecture change

`App.tsx` now owns three pieces of navigation state instead of one:

```ts
currentScreen: ScreenId | null
entryDirection: Direction        // which edge/menu the open screen came from
closing: boolean                 // true only during the return-to-Home animation
```

`openScreen(id, direction)` is the single entry point for opening or
switching to a section - called by `RadialMenu` (`direction: "radial"`),
`EdgeDrawers` (its own edge's direction), and `NexaIsland`'s quick-menu
navigation (`direction: "radial"`, since it has no literal edge to slide
from). Switching directly from one open section to another this way (via
the Island, without returning Home first) is an instant swap with a
fade-in of the new section - there is no animated exit of the old one in
that path, only entry. This was a deliberate simplification ("do not
over-engineer") rather than an oversight.

`closeScreen()` is the only path back to Home. It sets `closing: true`
(triggering the reverse-slide CSS, keyed off the *same* `entryDirection*
the screen opened with, so it always exits back the way it came),
then - after a timeout matching the CSS transition duration
(`CLOSE_ANIM_MS = 420`) - actually unmounts the screen and resets state.

## 5. How edge-screen transitions work

`ScreenShell` applies `data-direction={direction}` and
`data-closing={closing}` to its root element. CSS then does the rest:

- **Entry** uses a `@keyframes` animation per direction
  (`enter-from-top/bottom/left/right`, sliding the whole `translate`
  value in from fully off-screen to `0 0`; `enter-fade-scale` for
  `"radial"`, a calm opacity+scale fade rather than a slide since there
  is no edge to slide from).
- **Exit** (`data-closing="true"`) switches from `animation` to a plain
  CSS `transition` on the same properties (`translate`/`opacity`/
  `scale`), so setting the closing attribute smoothly reverses whatever
  the entry state was - no JS-driven animation, no library.
- This relies on `translate`/`scale`/`opacity` being independent,
  separately-animatable CSS properties (already used elsewhere in this
  codebase for the face and radial menu) so the keyframe-based entry and
  transition-based exit can coexist on the same element without
  conflicting.

Concretely: click the bottom "Chat" handle → `openScreen("chat",
"bottom")` → `ScreenShell` gets `data-direction="bottom"` → Chat's
`.screen-shell` animates `translate: 0 100% → 0 0` (slides up from the
bottom edge, filling the viewport). Click the NeXa Island → Home →
`closeScreen()` → `data-closing="true"` is added while `data-direction`
is still `"bottom"` → the same element transitions `translate: 0 0 → 0
100%` (slides back down off the bottom edge) → after 420ms it unmounts
and Home's large face reappears.

## 6. Confirmation: global SideNavigation removed

Verified by direct inspection, not just by removing the import:
- `src/components/SideNavigation.tsx` no longer exists in the repository
  (deleted, not just unused).
- `grep`-level check: `SideNavigation` is not referenced anywhere in
  `src/` after this change. This check initially failed - a stale doc
  comment in `ChatSidebar.tsx` still mentioned "the global
  SideNavigation (still rendered by ScreenShell...)" - fixed as part of
  this pass so the codebase and this report are both accurate, not just
  the import graph.
- Every full-screen section (`ChatScreen`, and every `PlaceholderScreen`
  instance: Today, Calendar, Goals, Projects, Money, Me, Insights,
  Settings, and the newly-added Notifications) renders through
  `ScreenShell`, which now renders only `NexaIsland` + content - no
  sidebar markup exists in that shell at all.

One small necessary addition while doing this: **Notifications** is now
a real `ScreenId` with its own `PlaceholderScreen` (milestone: M7),
because the brief requires its Top edge handle to open "an actual
screen/window," and there was previously no screen for it to open
(it had been a drawer-only concept). It is not on the radial ring
(spec keeps that at 8 items) but is reachable via its edge handle and via
the NeXa Island's quick-menu list.

## 7. Confirmation: ChatSidebar remains

`src/components/ChatSidebar.tsx` is untouched by this pass.
`ChatScreen.tsx` still renders `<ChatSidebar ... /> ` immediately
followed by `.chat-main` inside `.chat-screen`, exactly as the previous
pass built it - New Chat, a (visual-only) search field, and the demo chat
list with per-chat transcripts. The only change to `ChatScreen.tsx` in
this pass was adding the `direction`/`closing` props it now needs to pass
through to `ScreenShell` - the sidebar itself and its behavior are
unchanged.

## 8. Judgment calls made

- **"Me / Insights" (one bullet in the brief) became two separate edge
  handles** ("Me" and "Insights") rather than one handle that has to
  arbitrarily pick a single destination. With the preview-panel step
  removed, a single handle can only open one real screen, and picking
  one over the other for the user would be a worse interaction than just
  offering both directly. Flagged here for correction if a single
  combined handle was actually wanted.
- **Notifications gained a real placeholder screen** (§6) rather than
  being left as a dead-end handle, since the brief requires every edge
  handle to open "an actual screen/window."
- **Screen-to-screen navigation via the Island's "Menu" list does not
  animate an exit of the previous screen** (§4) - a deliberate
  simplification, not an oversight.

## 9. Validation results

| Check | Command | Result |
|---|---|---|
| Frontend build | `npm run build` (`tsc && vite build`) | **PASS** - 27 modules transformed, built in 175ms, zero TypeScript errors |
| Rust check | `cargo check --manifest-path src-tauri/Cargo.toml` | **PASS** - full dependency graph compiled and linked in 9.49s, zero errors |

No LifeOS-authored code caused any build failure. **No visual PASS is
claimed.** Only a human running `npm run tauri dev` can confirm the
slide directions, timing, and the Island's quick menu actually feel
"smooth, calm and premium" as required.

## 10. Remaining visual issues / things needing your look

- The four slide directions (top/bottom/left/right), their reverse-close
  motion, and the radial/Island fade-scale transition have not been seen
  running - only reasoned about and compiled.
- Whether 420ms (entry ~460ms, exit ~420ms) reads as "smooth, calm and
  premium" or needs tuning is a judgment call only visible in the real
  window.
- The "Me" vs "Insights" edge-handle split (§8) may not match what you
  actually wanted for that one bullet.
- Notifications now being a real (placeholder) screen, reachable from its
  edge handle and the Island's Menu list, is new - confirm this is the
  right scope rather than something lighter-weight.
- Everything already listed as pending in `docs/PROJECT_STATE.md` §8
  (local/cloud model integration, data-sensitivity classification, Goal
  Intelligence, Personal Knowledge, NeXa capability API, bilingual
  mechanism, LifeOS's own license, the still-unresolved one-off-report
  policy tension) is unchanged by this pass.
