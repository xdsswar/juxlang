# testing_demo

The testing framework's own demo (`JUX-TESTING-ADDENDUM` §TS).

`jux test` discovers `@Test` functions in a PROJECT — it searches upward for a
`jux.toml` rather than taking a file — so the demo is a project. As a loose
file it was unreachable by either command: `jux run` refused it for having no
`main`, and `jux test` had no way to be pointed at it.

```
cd examples/testing_demo
jux test
```
