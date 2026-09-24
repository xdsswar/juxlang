package dev.jux.intellij.resolve

import com.intellij.openapi.project.DumbService
import com.intellij.psi.PsiElement
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.completion.JuxAutoImport
import dev.jux.intellij.highlight.JuxKeywords
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.psi.JuxTypeParameter

/**
 * Which types a name needs an `import` for.
 *
 * A bare type name compiles when it is declared in the file, declared in the
 * file's own package or the root package, a type parameter in scope, part of
 * the prelude the compiler binds everywhere, or brought in by an import
 * (exact, grouped or wildcard). A name that is none of these, yet is declared
 * somewhere in the project or its libraries, is exactly the case Java's
 * "Import class" fix exists for: the fix is knowable, and the error is one
 * keystroke away from gone.
 */
object JuxImports {

    /**
     * The name a use site writes, when it is a bare (unqualified) type name:
     * a TYPE_REFERENCE with one identifier, or a REFERENCE_EXPRESSION used as a
     * type (`Auto.printT(...)`). Null for anything else.
     */
    fun bareTypeName(element: PsiElement): String? {
        return when (element.elementType) {
            E.TYPE_REFERENCE -> {
                val ids = element.node.getChildren(null)
                    .takeWhile { it.elementType !== E.TYPE_ARGUMENT_LIST }
                    .filter { it.elementType === T.IDENTIFIER }
                if (ids.size == 1) ids[0].text else null
            }
            E.REFERENCE_EXPRESSION -> {
                val ids = element.node.getChildren(null).filter { it.elementType === T.IDENTIFIER }
                if (ids.size == 1 && ids[0].text.firstOrNull()?.isUpperCase() == true) ids[0].text else null
            }
            else -> null
        }
    }

    /**
     * The types [name] could be imported as at [use], or an empty list when it
     * needs no import (or names nothing importable).
     */
    fun missingImportCandidates(use: PsiElement, name: String): List<JuxTypeDeclaration> {
        val file = use.containingFile as? JuxFile ?: return emptyList()
        val project = use.project
        if (DumbService.isDumb(project)) return emptyList()
        if (name in JuxKeywords.PRIMITIVES || name in JuxKeywords.BUILTINS || name == "Self") return emptyList()
        // A type parameter or a type declared in this file.
        val resolvedHere = JuxTypeEngine.resolveTypeName(use, name)
        if (resolvedHere is JuxTypeParameter) return emptyList()
        if (PsiTreeUtil.findChildrenOfType(file, JuxTypeDeclaration::class.java).any { it.name == name }) {
            return emptyList()
        }
        // A value-position name that is really a local, parameter or member.
        if (use.elementType === E.REFERENCE_EXPRESSION) {
            val target = JuxTypeEngine.resolveReferenceExpression(use)
            if (target != null && target !is JuxTypeDeclaration) return emptyList()
        }
        val candidates = JuxTypeIndex.typesNamed(project, name)
        if (candidates.isEmpty()) return emptyList()
        val ownPackage = JuxAutoImport.effectivePackageOfFile(file)
        for (c in candidates) {
            val pkg = JuxAutoImport.effectivePackageOf(c)
            // Reachable without an import: this very file, the same package, or
            // the root package. `needsNoImport` is the shared rule, so this
            // agrees with completion and with `juxc-lsp`'s `auto_import_for`.
            if (pkg.isEmpty() || JuxAutoImport.needsNoImport(file, c)) return emptyList()
            // `jux.std` is prepended to every unit by the compiler: never imported.
            if (pkg == "jux.std" || pkg.startsWith("jux.std.")) return emptyList()
            if (JuxAutoImport.isImported(file, "$pkg.$name", name)) return emptyList()
        }
        return candidates.filter { JuxHierarchy.typeVisibleFrom(it, ownPackage) }
    }

    /** `some.Truck` for a candidate type. */
    fun fqnOf(type: JuxTypeDeclaration): String {
        val pkg = JuxAutoImport.effectivePackageOf(type)
        return if (pkg.isEmpty()) type.name.orEmpty() else "$pkg.${type.name}"
    }
}
