# Document extraction fixtures

These fixtures contain only synthetic labels and values created specifically for the
Hummingbird test suite. They do not copy any user manual or third-party document.

- `register.xlsx`: two sheets, including a hidden sheet and an internal blank row.
- `dates.xlsx`: synthetic date, date-time, time, and duration cells.
- `register.xls`: two legacy workbook sheets; the second is hidden.
- `register-utf8-bom.csv`: comma-delimited UTF-8 BOM data with internal empty cells and rows.
- `register-gb18030.csv`: semicolon-delimited GB18030 data.
- `body-and-table.docx`: a paragraph, a table, then another paragraph.
- `three-pages.pdf`: text on pages one and three with an empty middle page.
- `image-only.pdf`: a valid PDF containing a raster Image XObject and no searchable text.
- `corrupt.pdf`: deliberately invalid bytes for PDF parse-error coverage.
- `corrupt.docx`: deliberately invalid package bytes for parse-error coverage.
- `corrupt.xls` and `corrupt.xlsx`: invalid workbook bytes for parse-error coverage.
