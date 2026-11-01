# Themes

Tide uses one semantic theme catalog for workbench roles, syntax rules, and terminal ANSI palettes. Fresh installs select Ultra Dusk.

## Built-in themes

1. Ultra Dusk
2. Catppuccin Frappé
3. Catppuccin Latte
4. Catppuccin Macchiato
5. Catppuccin Mocha
6. One Dark
7. Dracula
8. Nord
9. Tokyo Night
10. Gruvbox Dark
11. Solarized Dark
12. Solarized Light
13. Rosé Pine

The external palettes are mapped into Tide's semantic roles with attribution and pinned provenance. Syntax mappings retain each upstream theme family's own color intent rather than sharing a generic template.

## Select a theme

Open Settings, search for `workbench.colorTheme`, and choose a theme. Switching themes updates workbench surfaces and syntax highlighting without restarting or flashing a default palette.

## User themes

User themes live under the Tide configuration root. Incomplete themes receive deterministic fallback roles and bounded diagnostics. See [Theme authoring](../THEME_AUTHORING.md) for the role contract and [Theme attribution](../THEME_ATTRIBUTION.md) for bundled licenses.
