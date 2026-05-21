# Modern UI Design

`wyrd-diff` should feel like a focused desktop-grade developer tool: calm,
dense, precise, and fast to operate repeatedly.

## Visual Direction

- Prefer restrained surfaces, clear spacing, strong typography hierarchy, and
  deliberate contrast.
- Use cards for repeated items or genuinely framed tools, not for every page
  section.
- Keep app screens operational, not promotional.
- Avoid oversized hero sections, decorative blobs, decorative gradients, and
  marketing copy in workflow views.
- Make primary entities visible: repository, review session, diff, comment,
  note, decision, commit, trajectory record.

## Layout

- Keep navigation stable.
- Put status and primary actions close to the object they affect.
- Use split panes, tabs, filters, and drilldowns when they match review work.
- Maintain stable dimensions for toolbars, counters, diff panes, and lists so
  hover, loading, and count changes do not shift the layout.
- Use responsive constraints instead of viewport-scaled font sizes.

## Interaction States

- Provide hover, active, selected, disabled, loading, empty, error, and focus
  states for controls and data regions.
- Keep retry actions near failed content.
- Make active filters visible and removable.
- Provide copy actions for IDs, SHAs, paths, commands, and structured values
  users need elsewhere.

## Text

- Use concise operational labels.
- Avoid in-app explanation of obvious controls.
- Ensure the longest expected labels fit inside buttons, tabs, chips, and
  columns on mobile and desktop.
- Prefer specific empty/error text over generic failure messages.
