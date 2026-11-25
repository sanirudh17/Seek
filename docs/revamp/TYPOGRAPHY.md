# Seek UI typography

Figtree is the selected interface family, following the user's specimens and live review. Bundle the upright variable TTF locally; no system-font installation or runtime download. Use regular 400 for compact rows and controls, medium 500 for labels, and semibold 600 for headings. Code and document preview fonts remain independent; PDF rasterization is unchanged.

The TTF is 62,712 bytes. FontTools confirms typographic family Figtree and weight range 300–900. The source commit, SHA-256 and complete SIL OFL 1.1 are in app/assets/fonts. THIRD-PARTY-NOTICES includes the copyright and license; the notice generator now preserves additional asset/reference notices.

Recommendation among the supplied specimens: Figtree first for Seek's small labels and filenames; Switzer is the more neutral alternative. Brodine's broad specimen is more suitable for branding here. Geist Serif is a display face, unsuitable for dense file metadata. These are design judgments based on the supplied specimens and primary font information; only Figtree has been embedded and observed in the running app. No claim that all four were tested as installed fonts.

Sources: https://github.com/google/fonts/tree/a60a77e14f28abd4ef243a1b5dfc48df0cec5205/ofl/figtree ; https://www.fontshare.com/fonts/switzer ; https://www.myfonts.com/collections/brodine-font-artiveko ; https://vercel.com/blog/introducing-geist-serif .

Native live review was at the current Windows scale. Text is rendered at native GPUI sizes, without a scaled screenshot layer or text blur. Other DPI scales and every Unicode fallback remain unverified. The user accepted the Figtree appearance during live testing.
