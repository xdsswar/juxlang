# Internal working notes

These are development ledgers, not documentation. They record gap analyses,
audits and design deliberation as they happened, which means **they go stale**:
an item marked OPEN here has often been fixed since, and two files can disagree
with each other.

Verify against the compiler before trusting any status in this directory. The
user-facing entry points are [`../../README.md`](../../README.md) and
[`../../INSTALL.md`](../../INSTALL.md); the normative language definition is
[`../../Architecture/`](../../Architecture/).

| File | What it is |
|------|------------|
| `check-ups.md` | Structured review of the specification corpus, with per-risk verification blocks. |
| `jux-gaps.md` | Adversarial compiler gap scan (the N/C/H/O/P series). |
| `plugin-gap.md` | Audit of the IntelliJ plugin's extension-point coverage. |
| `considerations.md` | Early architecture audit; a historical snapshot, largely superseded. |
