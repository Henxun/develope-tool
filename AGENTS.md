<!-- BEGIN:nextjs-agent-rules -->
# This is NOT the Next.js you know

This version has breaking changes — APIs, conventions, and file structure may all differ from your training data. Read the relevant guide in `node_modules/next/dist/docs/` before writing any code. Heed deprecation notices.
<!-- END:nextjs-agent-rules -->

<!-- BEGIN:frontend-design-skill -->
# Skill: frontend-design (alias: fronted-design)

When implementing or modifying UI in this repository, apply the following frontend design skill by default:

1. Visual direction first
- Define a clear visual theme before coding (color palette, spacing rhythm, corner style, shadow style).
- Avoid generic boilerplate layouts and default-looking pages.

2. Typography and hierarchy
- Use purposeful font pairing (display + body + mono when needed).
- Keep heading/body contrast obvious and consistent.

3. Color and backgrounds
- Prefer layered backgrounds (soft gradients, radial accents, subtle glass/texture) over flat single-color canvases.
- Use CSS variables for semantic colors and keep contrast accessible.

4. Motion and interaction
- Add a few meaningful transitions (state change, hover, reveal) with restrained duration/easing.
- Avoid noisy animation and unnecessary motion.

5. Responsiveness
- Design mobile-first and validate desktop/tablet breakpoints.
- Ensure critical actions remain visible and reachable on small screens.

6. Consistency and reuse
- Reuse shared components/tokens when possible.
- Preserve existing visual language when extending current pages.

7. Delivery standard
- For UI tasks, include: design intent, key visual choices, and responsive behavior notes in the final report.
<!-- END:frontend-design-skill -->
