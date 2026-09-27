# Leaker

Leaker is an invoicing desktop app written in Jux. It uses egui through
eframe for the window and printpdf for the invoice PDFs. It was built to find
out where Jux's abstraction over Rust leaks. The findings are in
[LEAKS.md](LEAKS.md), which is the other half of this project.

The app is written the way a Java programmer would write it, with no
workarounds: egui's own containers with lambdas, widgets used directly, and a
real PDF crate. Two leaks are still open, and neither needs code worked
around it:

- egui's `Frame` cannot be named (L29), so panels are styled only through
  `Visuals`.
- Deprecated crate methods are not marked (L39).

## Build and run

You need the Jux toolchain from the repo root (`cargo build --release -p jux -p juxc`).
The first build generates the crate stubs into `.jux-stubs/`, which takes a
few minutes.

```
cd leaker
jux build
jux run                                    # the desktop app
jux run -- --screen customers              # open on a screen: overview | customers | products | invoices | INV-0003
jux run -- --size 800x600                  # start with a given window size (the layout is responsive)
jux run -- --demo-pdf                      # no window: seed the demo data, write invoices/INV-0001.pdf, print the path
jux run -- --demo-pdf --long               # the same with a 30-line invoice, which continues on page 2
```

The binary is `target/.rust-build/bin-leaker/target/release/leaker.exe`. PDFs
go to `invoices/` under the working directory.

## What it does

- **Overview**: four tiles for the amounts awaiting payment, overdue, in
  draft and collected, then the open invoices. Click a number to open one.
- **Customers**: a list, and a form in a side panel (below the list in a
  narrow window) to add, edit and delete customers. The form validates the
  name and email. A customer who still has invoices cannot be deleted.
- **Products**: add, edit and delete products. SKUs are unique, price and tax
  rate are validated, and the form shows a live "one unit with tax" preview.
  Changing a price does not change existing invoices, because each invoice line
  keeps a snapshot of the price.
- **Invoices**: the list, with a filter for All, Draft, Sent, Paid and
  Overdue. A new invoice is auto-numbered (`INV-0001`, ...).
- **The invoice editor** has:
  - a customer combo box, issue and due dates, and notes;
  - lines whose quantity, unit price and discount are drag values (type into
    one to enter a number), and a remove button;
  - an "Add line" product combo box;
  - live totals (subtotal, discount, tax, total) beside or below the lines;
  - buttons for Mark as Sent/Paid, Back to draft, Delete, and
    **Generate PDF**. The PDF's path is shown under the totals and on the
    status line.
- The side navigation can be resized by dragging its edge, and lists scroll.
  Below about 900 px the forms and totals move under the lists.

Money is a `long` number of cents (`Money`). Rates are basis points
(`Percent`). Rounding is half away from zero on every line's discount and tax.
No floating point touches an amount. The price drag value works on cents too,
and shows and reads them through `Money.format` and `Money.parse`.

## Layout

```
src/main.jux                      entry point: argument handling, --demo-pdf, window
src/leaker/model/                 Money, Percent, Date, Company, Customer, Product,
                                  LineItem, Invoice, InvoiceStatus, InvoiceTotals
src/leaker/service/               Store (in-memory repository, numbering), DemoData
src/leaker/pdf/                   PdfCanvas (a printpdf page with a top-left origin and
                                  text measured by the font) and InvoicePdf (the layout)
src/leaker/ui/                    App (panels and navigation), Session, the four screens,
                                  InvoiceEditor, InvoiceFilter, Palette, Cells
```
