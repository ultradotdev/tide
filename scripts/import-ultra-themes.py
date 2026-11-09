#!/usr/bin/env python3
"""Build Tide's Ultra Vivid and Ultra Broadcast themes from ultra-design-tokens.

Usage: scripts/import-ultra-themes.py <path to an ultra-design-tokens checkout>

Writes config/themes/ultra-<palette>.json and
config/syntax-themes/ultra-<palette>.json for each palette below, and prints
the source and output hashes to record in config/theme-provenance.json and
config/theme-syntax-provenance.json. Ultra Dusk is not built here: Tide's copy
comes from docs/theme-builder/ultra-dusk-design-response.json.
"""

import hashlib
import json
import sys
from pathlib import Path

PALETTES = ["vivid", "broadcast"]

# Tide theme key -> design token (dotted key in themes/<palette>.json). These
# are the keys Tide reads (crates/tide-ui/src/palette.rs); every value comes
# from a token, so a token missing from the design system stops the import.
COLORS = {
    "editor.background": "editor.background",
    "editor.foreground": "editor.foreground",
    "editor.selectionBackground": "editor.selection",
    "editor.lineHighlightBackground": "editor.line-highlight",
    "editorBracketMatch.background": "editor.bracket-match",
    "editorCursor.foreground": "editor.cursor",
    "editorLineNumber.foreground": "editor.line-number",
    "editorLineNumber.activeForeground": "editor.line-number-active",
    "editorWidget.background": "chrome.popover-background",
    "editorWidget.foreground": "chrome.menu-foreground",
    "editorWidget.border": "chrome.popover-border",
    "sideBar.background": "surface.substrate",
    "sideBarSectionHeader.background": "surface.crest",
    "sideBarSectionHeader.foreground": "text.primary",
    "panel.border": "border.default",
    "focusBorder": "chrome.pane-divider-active",
    "tab.activeBackground": "chrome.tab-active-background",
    "tab.activeForeground": "chrome.tab-active-foreground",
    "tab.inactiveBackground": "chrome.tab-inactive-background",
    "tab.inactiveForeground": "chrome.tab-inactive-foreground",
    "statusBar.background": "chrome.statusbar-background",
    "statusBar.foreground": "chrome.statusbar-foreground",
    "statusBarItem.errorBackground": "role.danger.fill",
    "input.background": "control.input-background",
    "input.foreground": "control.input-foreground",
    "button.background": "role.primary.fill",
    "button.foreground": "role.primary.on-fill",
    "successForeground": "role.confirm.fill",
    "list.activeSelectionBackground": "state.surface-selected",
    "list.activeSelectionForeground": "text.primary",
    "descriptionForeground": "text.secondary",
    "editorError.foreground": "diagnostic.error-foreground",
    "editorWarning.foreground": "diagnostic.warning-foreground",
    "gitDecoration.addedResourceForeground": "vcs.added-foreground",
    "gitDecoration.modifiedResourceForeground": "vcs.modified-foreground",
    "gitDecoration.deletedResourceForeground": "vcs.removed-foreground",
    "gitDecoration.untrackedResourceForeground": "vcs.untracked",
    "terminal.background": "terminal.background",
    "terminal.foreground": "terminal.foreground",
}

ANSI = [
    "Black", "Red", "Green", "Yellow", "Blue", "Magenta", "Cyan", "White",
    "BrightBlack", "BrightRed", "BrightGreen", "BrightYellow", "BrightBlue",
    "BrightMagenta", "BrightCyan", "BrightWhite",
]

def ansi_token(name):
    bright = name.startswith("Bright")
    base = name.removeprefix("Bright").lower()
    return f"terminal.ansi-{'bright-' if bright else ''}{base}"


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n")


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    source = Path(sys.argv[1])
    root = Path(__file__).resolve().parent.parent
    for palette in PALETTES:
        source_file = source / "themes" / f"{palette}.json"
        data = json.loads(source_file.read_text())
        tokens = data["tokens"]
        colors = {}
        for key, token in COLORS.items():
            if token not in tokens:
                sys.exit(f"{source_file}: no token {token} for {key}")
            colors[key] = tokens[token]
        for name in ANSI:
            colors[f"terminal.ansi{name}"] = tokens[ansi_token(name)]
        theme_id = f"ultra-{palette}"
        theme_file = root / "config" / "themes" / f"{theme_id}.json"
        syntax_file = root / "config" / "syntax-themes" / f"{theme_id}.json"
        write_json(theme_file, {
            "name": data["name"],
            "type": data["type"],
            "colors": colors,
            "tokenColors": data["tokenColors"],
        })
        write_json(syntax_file, {"themeId": theme_id, "tokenColors": data["tokenColors"]})
        print(f"{theme_id}: tokens v{data['version']}")
        print(f"  source {source_file.name} sha256 {sha256(source_file)}")
        print(f"  {theme_file.relative_to(root)} sha256 {sha256(theme_file)}")
        print(f"  {syntax_file.relative_to(root)} sha256 {sha256(syntax_file)}")


if __name__ == "__main__":
    main()
