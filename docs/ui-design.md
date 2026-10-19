# Millennium Blue — Accessible Cyber Console

An early-2000s technical console inspired by Uplink and digital-watch interfaces.
Clear panels and deliberate borders; no fake activity, blinking, or audit records.

| Role | Color |
| --- | --- |
| Background | `#080E1C` |
| Panel | `#15243A` |
| Primary text | `#E8EDF8` |
| Electric blue | `#628DFF` |
| Ice blue | `#B7D7FF` |
| Violet accent | `#675DFF` |
| Warning amber | `#FFD166` |

Primary text carries body content. Ice blue marks secondary headings. Electric blue
marks selected tabs with a `>` prefix and double border; violet is decorative only.
States use `[OK]` plus a label for success, `[!]` with amber for warnings/unavailable,
and `[ERROR]` with high-contrast primary text and a double border for future danger
panels. No current operational success or danger indicators are fabricated.
Meaning must remain clear when color is unavailable. Avoid text on violet fills.

Full layout (64×22 minimum): product header, three navigation tabs, bordered content,
and persistent keyboard hints. Dashboard lists actual implementation status; Help
lists keyboard commands; About identifies version/vendor. Smaller sizes show a compact
resize notice and exit hint. Resize never disables navigation or exit.

No animation is implemented; reduced motion is the default. Future motion must be
optional and respect a reduced-motion preference. True-color rendering varies by
terminal; text labels and border treatments preserve meaning on limited displays.
