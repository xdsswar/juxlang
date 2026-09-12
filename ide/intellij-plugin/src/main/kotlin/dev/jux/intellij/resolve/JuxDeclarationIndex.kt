package dev.jux.intellij.resolve

import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.psi.search.GlobalSearchScope
import com.intellij.util.indexing.DataIndexer
import com.intellij.util.indexing.DefaultFileTypeSpecificInputFilter
import com.intellij.util.indexing.FileBasedIndex
import com.intellij.util.indexing.FileBasedIndexExtension
import com.intellij.util.indexing.FileContent
import com.intellij.util.indexing.ID
import com.intellij.util.io.EnumeratorStringDescriptor
import com.intellij.util.io.KeyDescriptor
import com.intellij.util.io.VoidDataExternalizer
import dev.jux.intellij.JuxFileType

/**
 * A persistent, incremental index of every name a `.jux` file declares.
 *
 * ### Why this exists
 *
 * "Does any file in this project declare the name `Severity`?" is asked by
 * the unresolved-reference inspection for every identifier in the open file,
 * on every daemon pass, which is to say on every keystroke. Answering it by
 * walking PSI put the size of the whole project on the critical path of
 * typing one character.
 *
 * Caching each file's names against that file (see [JuxTypeIndex]) removed
 * the re-walking, but two costs remained, and both grow with the project:
 * the answer still needed every file's PSI to be loaded at least once, and
 * it was thrown away and rebuilt whenever the IDE restarted.
 *
 * A [FileBasedIndex] has neither. The platform maintains it incrementally,
 * re-indexing only files that actually changed, and persists it across
 * restarts. A lookup reads a map on disk; no PSI is loaded at all unless the
 * caller goes on to ask for the declaration itself, and then only for the
 * files the index named.
 *
 * ### Why not a stub index
 *
 * A `StubIndex` would also let the DECLARATION be returned without parsing,
 * not merely located. It needs every PSI class to be stub-based and every
 * indexed shape to have a serializer whose version is kept in step with the
 * parser -- and a stub that disagrees with the PSI it describes is one of the
 * few ways an IntelliJ plugin corrupts a project rather than merely being
 * slow. The cost here is one parse per changed file, in the background, and
 * that buys the same hot-path behaviour without that failure mode.
 *
 * ### What is indexed
 *
 * Every [dev.jux.intellij.psi.JuxNamedElement] except parameters and locals
 * -- the same set [JuxTypeIndex.symbolsIn] collects, and for the same
 * reason: a name worth finding from another file is one worth jumping to,
 * and a loop variable is not.
 */
class JuxDeclarationIndex : FileBasedIndexExtension<String, Void>() {

    override fun getName(): ID<String, Void> = NAME

    override fun getIndexer(): DataIndexer<String, Void, FileContent> =
        DataIndexer { content ->
            val result = HashMap<String, Void?>()
            val psi = content.psiFile
            for (decl in JuxTypeIndex.symbolsIn(psi)) {
                decl.name?.let { result[it] = null }
            }
            result
        }

    override fun getKeyDescriptor(): KeyDescriptor<String> = EnumeratorStringDescriptor.INSTANCE

    override fun getValueExternalizer(): VoidDataExternalizer = VoidDataExternalizer.INSTANCE

    /**
     * Bump when [getIndexer] changes what it records, so the platform
     * discards what it has rather than mixing two shapes of data.
     */
    override fun getVersion(): Int = 1

    override fun getInputFilter(): FileBasedIndex.InputFilter =
        DefaultFileTypeSpecificInputFilter(JuxFileType)

    /**
     * The indexer reads the file's text (as PSI), so the platform must
     * re-index on content change rather than on file identity alone. This is
     * also what the reference `RsAliasIndex` does; the `PsiDependentIndex`
     * marker some docs mention is internal and not shipped in the platform
     * this plugin builds against.
     */
    override fun dependsOnFileContent(): Boolean = true

    companion object {
        val NAME: ID<String, Void> = ID.create("jux.declaration.names")

        /**
         * True when [name] is declared somewhere in [scope].
         *
         * The question the unresolved-reference inspection actually asks,
         * asked directly. Enumerating every key in the index and testing
         * membership would be slower -- the inspection cares about the tens
         * of identifiers in one file, not the thousands of names in the
         * project -- and, worse, key enumeration is not reliably
         * scope-filtered, so names bled in from files outside the scope and
         * the inspection stopped flagging real mistakes.
         */
        fun isDeclared(name: String, project: Project, scope: GlobalSearchScope): Boolean =
            containingFiles(name, project, scope).isNotEmpty()

        /**
         * Whether the index holds anything for this project.
         *
         * Asked once per inspection run to decide between the index and the
         * PSI-walk fallback. A light test fixture that never ran indexing
         * gets the walk; a real project never pays for it.
         */
        fun hasData(project: Project): Boolean =
            FileBasedIndex.getInstance().getAllKeys(NAME, project).isNotEmpty()

        /**
         * The files declaring [name] -- the whole point of the index: a
         * caller that needs the declaration itself parses these, and nothing
         * else.
         */
        fun containingFiles(
            name: String,
            project: Project,
            scope: GlobalSearchScope,
        ): Collection<VirtualFile> =
            FileBasedIndex.getInstance().getContainingFiles(NAME, name, scope)
    }
}
