
# Stream Deck Plugin Image Guidelines

This document outlines the requirements and best practices for images used in the Star Citizen Stream Deck plugin, based on [Elgato's official guidelines](https://docs.elgato.com/guidelines/streamdeck/plugins/images-and-layouts).

## Quick Reference

| Image Type | Standard Size | High DPI Size | Format | Color Scheme |
|------------|---------------|---------------|--------|--------------|
| Plugin Icon | 256 × 256 px | 512 × 512 px | PNG | Full color |
| Category Icon | 28 × 28 px | 56 × 56 px | **SVG** or PNG | Monochrome white |
| Action Icon | 20 × 20 px | 40 × 40 px | **SVG** or PNG | Monochrome white |
| Key Icon (State) | 72 × 72 px | 144 × 144 px | **SVG**, PNG, or GIF | Any |

## Format Preference

**SVG is strongly preferred** for all icons except the main plugin icon:
- Scales perfectly across all device resolutions
- Smaller file sizes
- Ideal for dynamic content updates
- Future-proof for new Stream Deck hardware

Only use PNG when SVG is not feasible (e.g., complex raster graphics, photos).

## Plugin Icon

**File:** `plugin.png` + `plugin@2x.png`

**Requirements:**
- Standard: 256 × 256 px
- High DPI: 512 × 512 px
- Format: PNG only
- Full color allowed

**Guidelines:**
- Must accurately represent plugin functionality
- Should be recognizable at small sizes
- Avoid copyright infringement
- No offensive imagery

## Category Icons

**Naming:** `category-icon.svg` (preferred) or `category-icon.png` + `category-icon@2x.png`

**Requirements:**
- Standard: 28 × 28 px
- High DPI: 56 × 56 px
- Format: SVG or PNG
- **Monochrome white (#FFFFFF) on transparent background**

**Design Guidelines:**
- Use simple, clear shapes
- Ensure readability at small size
- Test visibility in both light and dark themes

## Action Icons

**Naming:** `action-{name}.svg` (preferred) or `action-{name}.png` + `action-{name}@2x.png`

**Requirements:**
- Standard: 20 × 20 px
- High DPI: 40 × 40 px
- Format: SVG or PNG
- **Monochrome white (#FFFFFF) on transparent background**

**Design Guidelines:**
- Very simple iconography (limited detail at 20px)
- High contrast shapes
- Avoid thin lines (minimum 2px stroke weight)
- Test legibility in action list

## Key Icons (State Images)

**Naming:** `key-{state}.svg` (preferred) or `key-{state}.png` + `key-{state}@2x.png`

**Requirements:**
- Standard: 72 × 72 px
- High DPI: 144 × 144 px
- Format: SVG (recommended), PNG, or GIF
- Color: Any (full color allowed)

**Best Practices:**
- **Use states to reflect action status:**
  - Default state: Normal/inactive
  - Active state: Action is active/running
  - Error state: Action failed or unavailable
- Limit programmatic updates to **10 per second maximum**
- SVG enables smooth dynamic updates (e.g., version labels)
- GIF for animations only—use programmatic updates for data changes

**Example State Design (Star Citizen plugin):**
- `key-version-live.svg`: LIVE channel indicator
- `key-version-ptu.svg`: PTU channel indicator
- `key-version-current.svg`: Shows current active version
- `key-settings.svg`: Settings icon

## High DPI Naming Convention

For PNG files, use the `@2x` suffix for high-DPI variants:

```
action-toggle.png       (20 × 20 px)
action-toggle@2x.png    (40 × 40 px)

key-default.png         (72 × 72 px)
key-default@2x.png      (144 × 144 px)
```

**SVG files do not need `@2x` variants** (they scale automatically).

## Color and Style Requirements

### Action and Category Icons (List View)
- **MUST** use monochrome white (#FFFFFF)
- **MUST** have transparent background
- **AVOID** colored icons—Stream Deck UI applies theme-appropriate backgrounds
- **AVOID** solid backgrounds—will clash with system theme

### Key Icons (Button Display)
- Full color allowed
- Consider dark backgrounds (Stream Deck buttons are typically dark)
- Ensure text/icons remain legible on black background
- Test with Stream Deck's brightness settings

## Design Best Practices

1. **Simplicity:** Icons should be instantly recognizable
2. **Consistency:** Maintain visual style across all plugin icons
3. **Accessibility:** Ensure sufficient contrast for visibility
4. **Scalability:** Test icons at actual display sizes
5. **Performance:** Optimize file sizes (especially for programmatic updates)

## User Feedback

When updating key images programmatically, complement with Stream Deck SDK feedback:

- `showAlert`: Briefly flash red X when action fails
- `showOk`: Briefly flash green checkmark on success (only if no visual state change)

**Example:** Toggle action might change key icon state (visual feedback) + `showOk` for confirmation.

## Touch Strip Layouts (Future)

If implementing Stream Deck+ dial layouts:

**Size:** 200 × 100 px

**Requirements:**
- Interactive elements: minimum 35 × 35 px touch target
- All content within boundaries
- Avoid cramped layouts
- Limit updates to 10/second

## File Organization

Recommended structure for this plugin:

```
images/
├── README.md                 (this file)
├── plugin.png               (256×256)
├── plugin@2x.png            (512×512)
├── category-icon.svg        (28×28, preferred)
├── actions/
│   ├── set-version.svg      (20×20)
│   ├── toggle-version.svg   (20×20)
│   ├── current-version.svg  (20×20)
│   └── settings.svg         (20×20)
└── keys/
    ├── version-live.svg     (72×72)
    ├── version-ptu.svg      (72×72)
    ├── version-eptu.svg     (72×72)
    └── settings.svg         (72×72)
```

## Testing Checklist

Before finalizing images:

- [ ] All icons use correct dimensions
- [ ] High DPI variants provided for PNG files
- [ ] Action/category icons are monochrome white on transparent
- [ ] Icons tested in Stream Deck software
- [ ] SVG files validated (no errors)
- [ ] File sizes optimized
- [ ] Icons legible at actual display size
- [ ] Visual consistency across all icons

## Resources

- [Elgato Official Guidelines](https://docs.elgato.com/guidelines/streamdeck/plugins/images-and-layouts)
- [Stream Deck SDK Documentation](https://docs.elgato.com/sdk/plugins/overview)
- [SVG Optimization Tools](https://jakearchibald.github.io/svgomg/)
