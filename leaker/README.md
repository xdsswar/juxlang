# Leaker

Leaker is an invoicing desktop app written in Jux. It uses egui through
eframe for the window and writes invoice PDFs. It was built to find out
where Jux's abstraction over Rust leaks. The findings are in
[LEAKS.md](LEAKS.md), which is the other half of this project.

## Build and run

You need the Jux toolchain from the repo root (`cargo build --release -p jux -p juxc`).
Generating the crate stubs also needs a nightly `rustdoc`.

```
cd leaker
jux build                                  # first build also generates .jux-stubs/ (slow)
jux run                                    # the desktop app
jux run -- --screen customers              # open on a screen: overview | customers | products | invoices | INV-0003
jux run -- --size 800x600                  # start with a given window size (the layout is responsive)
jux run -- --demo-pdf                      # no window: seed the demo data, write invoices/INV-0001.pdf, print the path
jux run -- --demo-pdf --long               # the same with a 30-line invoice, to check that the table continues on page 2
```

The binary is `target/.rust-build/bin-leaker/target/release/leaker.exe`. PDFs
go to `invoices/` under the working directory.

## What it does

- **Overview**: outstanding, overdue, draft and collected amounts, plus the
  open invoices (click one to open it).
- **Customers**: a list with a form beside it (or below it in a narrow window)
  to add, edit and delete customers. The form validates the name and email. A
  customer who still has invoices cannot be deleted.
- **Products**: add, edit and delete products. SKUs are unique, price and tax
  rate are validated, and the form shows a live "one unit with tax" preview.
  Changing a price does not change existing invoices, because each invoice line
  keeps a snapshot of the price.
- **Invoices**: the list, filterable by status, including Overdue. A new
  invoice is auto-numbered (`INV-0001`, ...). The editor has a customer combo
  box, issue and due dates, notes, and lines with -/+ quantity, editable unit
  price and discount %, and a remove button. It also has an "Add line" product
  combo box, live totals (subtotal, discount, tax, total), and buttons for
  Mark as Sent/Paid, Back to draft, Delete, and **Generate PDF**. The PDF's path
  is shown in the totals card and on the status line.
- The side navigation can be resized by dragging its edge, lists scroll, and
  the layouts rearrange below about 900 px.

Money is a `long` number of cents (`Money`). Rates are basis points
(`Percent`). Rounding is half away from zero on every line's discount and tax.
No floating point touches an amount.

## Layout

```
src/main.jux                      entry point: argument handling, --demo-pdf, window
src/leaker/model/                 Money, Percent, Date, Company, Customer, Product,
                                  LineItem, Invoice, InvoiceStatus, InvoiceTotals
src/leaker/service/               Store (in-memory repository, numbering), DemoData
src/leaker/pdf/                   PdfDocument/PdfPage/PdfColor/FontMetrics (a small
                                  PDF 1.4 writer in pure Jux) and InvoicePdf (the layout)
src/leaker/ui/                    App, Session, the four screens, InvoiceEditor,
                                  LineDraft, and the home-made toolkit: Gui, Form,
                                  Table, ScrollPane, Palette
```

Why the app has its own toolkit (`Gui`, `ScrollPane`, `Table`), and why the
PDF writer is hand-written instead of using a crate, is explained in LEAKS.md
(L1, L3, L19 to L21).
