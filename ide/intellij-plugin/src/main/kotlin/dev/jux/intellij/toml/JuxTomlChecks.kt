package dev.jux.intellij.toml

/**
 * The editor's checks of a `jux.toml` against the workspace rules of
 * JUX-BUILD-SYSTEM-ADDENDUM §B.7, as data (a range, a message, a level) so
 * they unit-test without a fixture. [JuxManifestInspection] turns them into
 * highlights.
 *
 * Each problem is one the build would also stop at or warn about, caught
 * while typing: an unknown `[workspace]` key, a member directory that holds
 * no `jux.toml`, a pattern that matches nothing, a `default-members` entry
 * that is not a member, and a `key.workspace = true` whose key the root never
 * declares.
 */
object JuxTomlChecks {

    enum class Level { ERROR, WARNING, WEAK_WARNING }

    data class Problem(val start: Int, val end: Int, val message: String, val level: Level)

    /**
     * Check [text], the `jux.toml` in directory [dir] (a view of that
     * directory, used to resolve its own `members`). [rootText] is the
     * manifest of the workspace root this one belongs to (the nearest
     * `jux.toml` with a `[workspace]` table at or above it), null when there
     * is none.
     */
    fun check(text: String, dir: JuxTomlModel.DirView, rootText: String?): List<Problem> {
        val out = ArrayList<Problem>()
        val entries = JuxTomlModel.entries(text)

        // ---- [workspace] keys and member lists -------------------------------
        for (e in entries.filter { it.table == "workspace" }) {
            if (e.head !in JuxTomlModel.WORKSPACE_KEYS && e.head != "package" && e.head != "dependencies") {
                out.add(
                    Problem(
                        e.keyStart, e.headEnd,
                        "Unknown key `${e.head}` in [workspace]: it takes members, exclude and default-members",
                        Level.WARNING,
                    ),
                )
                continue
            }
            if (e.head in JuxTomlModel.WORKSPACE_KEYS && !e.value.startsWith("[")) {
                out.add(Problem(e.valueStart, e.valueEnd, "`${e.head}` is a list of paths: `${e.head} = [\"...\"]`", Level.ERROR))
            }
        }
        if (JuxTomlModel.isWorkspaceRoot(text)) checkMembers(text, entries, dir, out)

        // ---- key.workspace = true inheritance --------------------------------
        for (e in entries.filter { it.inheritsFromWorkspace }) {
            val where = when (e.table) {
                "package" -> "[workspace.package]"
                "dependencies" -> "[workspace.dependencies]"
                else -> {
                    out.add(
                        Problem(
                            e.keyStart, e.valueEnd,
                            "Only [package] and [dependencies] entries can take their value from the workspace",
                            Level.WARNING,
                        ),
                    )
                    continue
                }
            }
            if (e.table == "package" && e.head == "name") {
                out.add(Problem(e.keyStart, e.headEnd, "A member's `name` is its own; it cannot come from the workspace", Level.ERROR))
                continue
            }
            if (rootText == null) {
                out.add(
                    Problem(
                        e.keyStart, e.valueEnd,
                        "`${e.head}` inherits from the workspace, but no jux.toml with [workspace] is at or above this one",
                        Level.ERROR,
                    ),
                )
                continue
            }
            val offered = if (e.table == "package") {
                JuxTomlModel.workspacePackageKeys(rootText)
            } else {
                JuxTomlModel.workspaceDependencyNames(rootText)
            }
            if (e.head !in offered) {
                out.add(Problem(e.keyStart, e.headEnd, "`${e.head}` is not declared in the workspace root's $where", Level.ERROR))
            }
        }
        return out
    }

    /** `members` entries that resolve to nothing, and `default-members` that are not members. */
    private fun checkMembers(
        text: String,
        entries: List<JuxTomlModel.Entry>,
        dir: JuxTomlModel.DirView,
        out: MutableList<Problem>,
    ) {
        val membersEntry = entries.firstOrNull { it.table == "workspace" && it.key == "members" }
        val excludeEntry = entries.firstOrNull { it.table == "workspace" && it.key == "exclude" }
        val defaultEntry = entries.firstOrNull { it.table == "workspace" && it.key == "default-members" }
        val members = membersEntry?.let { JuxTomlModel.stringArray(it.value) }.orEmpty()
        val exclude = excludeEntry?.let { JuxTomlModel.stringArray(it.value) }.orEmpty()

        membersEntry?.let { e ->
            for (m in members) {
                val norm = JuxTomlModel.normalizePath(m)
                val range = stringRange(text, e, m) ?: continue
                if (JuxTomlModel.hasWildcard(norm)) {
                    if (JuxTomlModel.expandPattern(dir, norm).none(dir::hasManifest)) {
                        out.add(Problem(range.first, range.second, "`$m` matches no directory with a jux.toml", Level.WEAK_WARNING))
                    }
                } else if (!dir.hasManifest(norm)) {
                    val why = if (dir.isDirectory(norm)) "has no jux.toml" else "does not exist"
                    out.add(Problem(range.first, range.second, "Workspace member `$m` $why", Level.WARNING))
                }
            }
        }
        defaultEntry?.let { e ->
            val expanded = JuxTomlModel.expandMembers(dir, members, exclude)
            for (d in JuxTomlModel.stringArray(e.value)) {
                if (expanded.none { JuxTomlModel.pathMatches(d, it) }) {
                    val range = stringRange(text, e, d) ?: continue
                    out.add(Problem(range.first, range.second, "`$d` is not a workspace member", Level.WARNING))
                }
            }
        }
    }

    /** The offsets of the quoted string [s] (quotes included) inside [e]'s value. */
    fun stringRange(text: String, e: JuxTomlModel.Entry, s: String): Pair<Int, Int>? {
        val span = text.substring(e.valueStart, e.valueEnd.coerceAtMost(text.length))
        for (q in listOf("\"$s\"", "'$s'")) {
            val at = span.indexOf(q)
            if (at >= 0) return (e.valueStart + at) to (e.valueStart + at + q.length)
        }
        return null
    }
}
