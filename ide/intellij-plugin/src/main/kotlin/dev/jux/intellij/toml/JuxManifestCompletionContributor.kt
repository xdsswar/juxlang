package dev.jux.intellij.toml

import com.intellij.codeInsight.completion.CompletionContributor
import com.intellij.codeInsight.completion.CompletionParameters
import com.intellij.codeInsight.completion.CompletionResultSet
import com.intellij.codeInsight.completion.InsertHandler
import com.intellij.codeInsight.lookup.LookupElement
import com.intellij.codeInsight.lookup.LookupElementBuilder
import com.intellij.icons.AllIcons
import com.intellij.openapi.vfs.VirtualFile

/**
 * Completion in `jux.toml` for the workspace keys of
 * JUX-BUILD-SYSTEM-ADDENDUM §B.7:
 *
 *  - in `[workspace]`: `members`, `exclude`, `default-members`;
 *  - inside those arrays: member directories (a directory holding a
 *    `jux.toml`, and a wildcard pattern for a directory of them), and for
 *    `default-members` the members the root actually has;
 *  - in a member's `[package]`: `key.workspace = true` for every key the
 *    root's `[workspace.package]` declares, and in `[dependencies]` the same
 *    for `[workspace.dependencies]`;
 *  - on a `[` line: the manifest's table names.
 *
 * Registered for every language (like [JuxManifestInspection]) and
 * skipped by file name, so it works with or without the TOML plugin.
 */
class JuxManifestCompletionContributor : CompletionContributor() {

    override fun fillCompletionVariants(parameters: CompletionParameters, result: CompletionResultSet) {
        val file = parameters.originalFile
        if (!JuxManifestFiles.isManifest(file)) return
        val vf = file.virtualFile ?: return
        val text = file.text
        val offset = parameters.offset.coerceIn(0, text.length)
        val lineStart = text.lastIndexOf('\n', offset - 1) + 1
        val linePrefix = text.substring(lineStart, offset)

        // `[wor` on a header line: table names.
        HEADER_PREFIX.matchEntire(linePrefix)?.let { m ->
            val set = result.withPrefixMatcher(m.groupValues[1])
            TABLES.forEach { set.addElement(LookupElementBuilder.create(it).withIcon(AllIcons.Nodes.DataTables)) }
            result.stopHere()
            return
        }

        val table = JuxTomlModel.tableAt(text, offset)

        // Inside a string of a [workspace] path list.
        arrayKeyAt(text, offset, linePrefix)?.let { key ->
            val prefix = linePrefix.substringAfterLast('"').substringAfterLast('\'')
            val set = result.withPrefixMatcher(prefix)
            pathCandidates(vf, text, key).forEach {
                set.addElement(LookupElementBuilder.create(it).withIcon(AllIcons.Nodes.Folder).withTypeText(key, true))
            }
            result.stopHere()
            return
        }

        // A key position: nothing but a (partial) key before the caret.
        if (!KEY_PREFIX.matches(linePrefix)) return
        val set = result.withPrefixMatcher(linePrefix.trim())
        when (table) {
            "workspace" -> JuxTomlModel.WORKSPACE_KEYS.forEach { key ->
                set.addElement(
                    LookupElementBuilder.create(key)
                        .withTypeText("[workspace]", true)
                        .withTailText(" = [...]", true)
                        .withInsertHandler(ARRAY_VALUE),
                )
            }
            "package", "dependencies" -> {
                val root = JuxManifestFiles.workspaceRoot(vf) ?: return
                val rootText = if (root == vf) text else JuxManifestFiles.textOf(root)
                val present = JuxTomlModel.entries(text).filter { it.table == table }.map { it.head }.toSet()
                val offered = if (table == "package") {
                    JuxTomlModel.workspacePackageKeys(rootText)
                } else {
                    JuxTomlModel.workspaceDependencyNames(rootText)
                }
                for (key in offered.filter { it !in present }) {
                    set.addElement(inheritElement(key, table))
                }
            }
        }
    }

    /** `edition.workspace = true`, found by typing `edition`. */
    private fun inheritElement(key: String, table: String): LookupElement {
        val written = if (key.contains('.') || key.contains('-')) "\"$key\"" else key
        return LookupElementBuilder.create("$written.workspace = true")
            .withLookupString(key)
            .withPresentableText("$key.workspace = true")
            .withTypeText(if (table == "package") "[workspace.package]" else "[workspace.dependencies]", true)
            .withIcon(AllIcons.Nodes.Parameter)
    }

    /**
     * The `[workspace]` array key ([JuxTomlModel.WORKSPACE_KEYS]) whose value
     * holds [offset], when the caret is inside one of its strings.
     */
    private fun arrayKeyAt(text: String, offset: Int, linePrefix: String): String? {
        // Inside a string: an odd number of quotes before the caret on this line.
        if (linePrefix.count { it == '"' } % 2 == 0 && linePrefix.count { it == '\'' } % 2 == 0) return null
        ONE_LINE_ARRAY.find(linePrefix)?.let { return it.groupValues[1] }
        // A multi-line array: the entry whose value spans the caret.
        return JuxTomlModel.entries(text)
            .firstOrNull { it.table == "workspace" && it.head in JuxTomlModel.WORKSPACE_KEYS && offset in it.valueStart..it.valueEnd }
            ?.head
    }

    /** Candidate paths for a `[workspace]` array, relative to the manifest's directory. */
    private fun pathCandidates(manifest: VirtualFile, text: String, key: String): List<String> {
        val dir = manifest.parent ?: return emptyList()
        val view = JuxManifestFiles.VirtualDirView(dir)
        if (key == "default-members") {
            return JuxTomlModel.expandMembers(
                view,
                JuxTomlModel.workspaceArray(text, "members"),
                JuxTomlModel.workspaceArray(text, "exclude"),
            )
        }
        val out = LinkedHashSet<String>()
        fun walk(rel: String, depth: Int) {
            if (depth > 3) return
            for (child in view.childDirs(rel).sorted()) {
                if (child.startsWith('.') || child == "target" || child == "src" || child == "test") continue
                val path = if (rel.isEmpty()) child else "$rel/$child"
                if (view.hasManifest(path)) out.add(path)
                walk(path, depth + 1)
            }
            // A directory with two or more packages in it offers `dir/*`.
            if (rel.isNotEmpty() && view.childDirs(rel).count { view.hasManifest("$rel/$it") } >= 2) out.add("$rel/*")
        }
        walk("", 0)
        return out.toList()
    }

    private companion object {
        val HEADER_PREFIX = Regex("""^\s*\[\[?\s*([A-Za-z0-9_.\-]*)$""")
        val KEY_PREFIX = Regex("""^\s*[A-Za-z0-9_.\-"]*$""")
        val ONE_LINE_ARRAY = Regex("""^\s*(members|exclude|default-members)\s*=\s*\[""")

        /** Tables a Jux manifest declares (JUX-BUILD-SYSTEM-ADDENDUM §B.2-§B.9). */
        val TABLES = listOf(
            "package", "dependencies", "lib", "bin", "build", "features",
            "workspace", "workspace.package", "workspace.dependencies",
            "profile.dev", "profile.release", "profile.test", "profile.bench",
        )

        /** `members` -> `members = [|]`. */
        val ARRAY_VALUE = InsertHandler<LookupElement> { ctx, _ ->
            val doc = ctx.document
            val at = ctx.tailOffset
            val rest = doc.charsSequence.subSequence(at, minOf(doc.textLength, at + 4)).toString()
            if (!rest.trimStart().startsWith("=")) {
                doc.insertString(at, " = []")
                ctx.editor.caretModel.moveToOffset(at + 4)
            }
        }
    }
}
