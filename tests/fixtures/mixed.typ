#set page(width: 5in, height: 4in, margin: 0.5in)
#set text(size: 11pt)

= Mixed Content

Some *bold text* and _italic text_ and `monospace text` in a paragraph.

#table(
  columns: (1fr, 1fr, 1fr),
  [Name], [Value], [Unit],
  [Width], [120], [mm],
  [Height], [80], [mm],
  [Depth], [45], [mm],
)

#v(0.5em)

A final paragraph after the table, with a footnote.#footnote[This is a footnote.]
