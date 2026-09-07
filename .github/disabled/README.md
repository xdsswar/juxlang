# Disabled workflows

GitHub only runs what is in `.github/workflows/`. Anything here is inert: no
runs, no minutes, no emails.

To turn CI back on, move the file back:

```
git mv .github/disabled/ci.yml .github/workflows/ci.yml
```

The workflow itself is unchanged, so it will pick up where it left off.
