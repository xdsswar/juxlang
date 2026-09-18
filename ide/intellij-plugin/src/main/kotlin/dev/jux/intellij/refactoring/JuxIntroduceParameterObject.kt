package dev.jux.intellij.refactoring

import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.actionSystem.DataContext
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.openapi.ui.DialogWrapper
import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.elementType
import com.intellij.refactoring.RefactoringActionHandler
import com.intellij.ui.components.JBCheckBox
import com.intellij.ui.components.JBLabel
import com.intellij.ui.components.JBTextField
import com.intellij.util.ui.FormBuilder
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxMethodDeclaration
import dev.jux.intellij.resolve.JuxHierarchy
import dev.jux.intellij.resolve.JuxSubtypes
import dev.jux.intellij.resolve.JuxTypeIndex
import javax.swing.JComponent

/**
 * Introduce Parameter Object: several parameters of a method become one
 * `record`, the body reads them as `param.name`, and every call passes
 * `new Record(...)`.
 *
 * A record is exactly the shape Java makes a class for by hand here: final
 * components, a canonical constructor, `==`, hash and a string form derived.
 * It goes in its own `<Name>.jux` next to the method's file, in the same
 * package (spec §3.1), and callers in other packages get an import.
 *
 * Refused for a method in an override family (every override would change,
 * which is Change Signature's work), for a chosen parameter the body assigns
 * (a record component is final), and for one with a default value.
 */
class JuxIntroduceParameterObjectHandler : RefactoringActionHandler {

    override fun invoke(project: Project, editor: Editor?, file: PsiFile?, dataContext: DataContext?) {
        val element = dataContext?.let { CommonDataKeys.PSI_ELEMENT.getData(it) }
            ?: file?.findElementAt(editor?.caretModel?.offset ?: return)
            ?: return
        var e: PsiElement? = element
        while (e != null && e !is JuxMethodDeclaration && e !is PsiFile) e = e.parent
        val method = e as? JuxMethodDeclaration
        if (method == null) {
            JuxRefactoringInput.refuse(project, editor, TITLE, "Place the caret on a method.")
            return
        }
        invoke(project, arrayOf(method), dataContext)
    }

    override fun invoke(project: Project, elements: Array<out PsiElement>, dataContext: DataContext?) {
        val method = elements.firstOrNull() as? JuxMethodDeclaration ?: return
        val editor = dataContext?.let { CommonDataKeys.EDITOR.getData(it) }
        val params = JuxHierarchy.parameters(method)
        if (params.size < 2) {
            JuxRefactoringInput.refuse(project, editor, TITLE, "`${method.name}` needs at least two parameters to group.")
            return
        }
        val defaultName = (method.name ?: "Method").replaceFirstChar { it.uppercase() } + "Params"
        val (recordName, chosen) = if (ApplicationManager.getApplication().isUnitTestMode) {
            val name = JuxRefactoringInput.answers[TITLE] ?: defaultName
            val picked = JuxRefactoringInput.answers[PARAMETERS]?.split(',')?.map { it.trim() }
                ?: params.mapNotNull { JuxRefactoringUtil.nameOf(it) }
            name to params.indices.filter { JuxRefactoringUtil.nameOf(params[it]) in picked }
        } else {
            val dialog = Dialog(project, defaultName, params.mapNotNull { JuxRefactoringUtil.nameOf(it) })
            if (!dialog.showAndGet()) return
            dialog.recordName() to dialog.chosen()
        }
        val change = try {
            plan(method, recordName, chosen)
        } catch (r: JuxExtractMethodHandler.Refusal) {
            JuxRefactoringInput.refuse(project, editor, TITLE, r.message!!)
            return
        }
        WriteCommandAction.writeCommandAction(project).withName(TITLE).run<RuntimeException> {
            val directory = method.containingFile.containingDirectory
            val created = directory.createFile("$recordName.jux")
            val document = com.intellij.psi.PsiDocumentManager.getInstance(project).getDocument(created)
            document?.setText(change.recordText)
            document?.let { com.intellij.psi.PsiDocumentManager.getInstance(project).commitDocument(it) }
            JuxRefactoringUtil.applyEdits(project, change.edits)
        }
    }

    /** What the refactoring writes: the record file's text and the edits elsewhere. */
    internal class Change(val recordText: String, val edits: List<JuxRefactoringUtil.Edit>)

    /** The checkbox dialog the IDE shows. */
    private class Dialog(project: Project, defaultName: String, names: List<String>) : DialogWrapper(project, true) {
        private val nameField = JBTextField(defaultName)
        private val boxes = names.map { JBCheckBox(it, true) }

        init {
            title = TITLE
            init()
        }

        override fun createCenterPanel(): JComponent {
            val form = FormBuilder.createFormBuilder().addLabeledComponent(JBLabel("Record name:"), nameField)
            form.addComponent(JBLabel("Parameters to group:"))
            boxes.forEach { form.addComponent(it) }
            return form.panel
        }

        override fun doValidate() = JuxRefactoringInput.identifierProblem(nameField.text.trim())
            ?.let { com.intellij.openapi.ui.ValidationInfo(it, nameField) }
            ?: if (chosen().size < 2) com.intellij.openapi.ui.ValidationInfo("Choose at least two parameters.") else null

        fun recordName() = nameField.text.trim()
        fun chosen() = boxes.indices.filter { boxes[it].isSelected }
    }

    companion object {
        const val TITLE = "Introduce Parameter Object"

        /** The test-answer key for the chosen parameters, comma-separated. */
        const val PARAMETERS = "$TITLE: parameters"

        /** Plan the refactoring for the parameters at [chosen], or throw a refusal. */
        internal fun plan(method: JuxMethodDeclaration, recordName: String, chosen: List<Int>): Change {
            val name = method.name ?: throw refusal("The method has no name.")
            JuxRefactoringInput.identifierProblem(recordName)?.let { throw refusal(it) }
            if (chosen.size < 2) throw refusal("Choose at least two parameters.")
            val owner = JuxHierarchy.enclosingType(method)
            if (owner != null && (JuxHierarchy.findSuperMethod(owner, name, JuxHierarchy.arity(method)) != null ||
                    JuxSubtypes.overridingMethods(method).isNotEmpty())
            ) throw refusal("`$name` overrides or is overridden. Change the family with Change Signature.")
            if (JuxTypeIndex.findType(method, recordName) != null) throw refusal("A type named `$recordName` already exists.")
            val file = method.containingFile
            if (file.containingDirectory?.findFile("$recordName.jux") != null) throw refusal("`$recordName.jux` already exists.")

            val params = JuxHierarchy.parameters(method)
            val picked = chosen.map { params[it] }
            for (p in picked) {
                if (p.node.findChildByType(T.EQ) != null) throw refusal("`${JuxRefactoringUtil.nameOf(p)}` has a default value. Remove it first.")
                if (JuxRefactoringUtil.declaredTypeText(p) == null) throw refusal("`${JuxRefactoringUtil.nameOf(p)}` has no written type.")
            }
            val body = JuxRefactoringUtil.body(method)
            val uses = body?.let { JuxRefactoringUtil.variableUses(it) } ?: emptyList()
            uses.firstOrNull { (decl, use) -> decl in picked && isWritten(use) }?.let { (decl, _) ->
                throw refusal("The body assigns `${JuxRefactoringUtil.nameOf(decl)}`, and a record component is final.")
            }

            val paramName = recordName.replaceFirstChar { it.lowercase() }.let { base ->
                val taken = params.mapNotNull { JuxRefactoringUtil.nameOf(it) }.toSet() - picked.mapNotNull { JuxRefactoringUtil.nameOf(it) }.toSet()
                if (base in taken) "${base}Obj" else base
            }
            val edits = ArrayList<JuxRefactoringUtil.Edit>()
            // The declaration: the chosen parameters become one, where the first was.
            val list = JuxRefactoringUtil.parameterList(method)!!
            val newParams = ArrayList<String>()
            params.forEachIndexed { i, p ->
                if (i == chosen.first()) newParams += "$recordName $paramName"
                else if (i !in chosen) newParams += p.text.trim()
            }
            edits += JuxRefactoringUtil.Edit(file, list.textRange, newParams.joinToString(", ", "(", ")"))
            // The body: each chosen parameter is read through the record.
            if (body != null) {
                edits += JuxRefactoringUtil.renameEdits(body, picked.associateWith { "$paramName.${JuxRefactoringUtil.nameOf(it)}" })
            }
            // The calls: the chosen arguments are wrapped into the record.
            val pkg = packageOf(file)
            for (call in JuxRefactoringUtil.callsOf(method)) {
                val args = JuxRefactoringUtil.arguments(call)
                val argList = JuxRefactoringUtil.argumentList(call) ?: continue
                val parts = ArrayList<JuxRefactoringUtil.Part>()
                parts += JuxRefactoringUtil.Part.Lit("(")
                var first = true
                fun sep() { if (!first) parts += JuxRefactoringUtil.Part.Lit(", "); first = false }
                args.forEachIndexed { i, arg ->
                    when {
                        i == chosen.first() -> {
                            sep()
                            parts += JuxRefactoringUtil.Part.Lit("new $recordName(")
                            chosen.forEachIndexed { k, idx ->
                                if (k > 0) parts += JuxRefactoringUtil.Part.Lit(", ")
                                args.getOrNull(idx)?.let { parts += JuxRefactoringUtil.Part.Slice(it.textRange) }
                            }
                            parts += JuxRefactoringUtil.Part.Lit(")")
                        }
                        i !in chosen -> { sep(); parts += JuxRefactoringUtil.Part.Slice(arg.textRange) }
                    }
                }
                parts += JuxRefactoringUtil.Part.Lit(")")
                edits += JuxRefactoringUtil.Edit(call.containingFile, argList.textRange, parts)
                importEdit(call.containingFile, pkg, recordName)?.let { edits += it }
            }
            val components = picked.joinToString(", ") { "${JuxRefactoringUtil.declaredTypeText(it)} ${JuxRefactoringUtil.nameOf(it)}" }
            val header = if (pkg.isEmpty()) "" else "package $pkg;\n\n"
            return Change("${header}public record $recordName($components) {}\n", edits.distinct())
        }

        private fun refusal(message: String) = JuxExtractMethodHandler.Refusal(message)

        private fun isWritten(use: PsiElement): Boolean {
            val parent = use.parent ?: return false
            return when (parent.elementType) {
                E.ASSIGNMENT_EXPRESSION -> parent.firstChild === use
                E.POSTFIX_EXPRESSION, E.UNARY_EXPRESSION ->
                    parent.node.findChildByType(T.PLUS_PLUS) != null || parent.node.findChildByType(T.MINUS_MINUS) != null
                else -> false
            }
        }

        /** The `package` a file declares, `""` for none. */
        fun packageOf(file: PsiFile): String =
            file.node.findChildByType(E.PACKAGE_STATEMENT)?.findChildByType(E.QUALIFIED_NAME)?.text?.replace(Regex("\\s"), "") ?: ""

        /** An `import pkg.Name;` for [file] when it is in another package and does not see [name] yet. */
        internal fun importEdit(file: PsiFile, pkg: String, name: String): JuxRefactoringUtil.Edit? {
            if (pkg.isEmpty() || packageOf(file) == pkg) return null
            val imports = file.children.filter { it.elementType === E.IMPORT_STATEMENT }
            val covered = imports.any {
                val text = it.text.replace(Regex("\\s"), "")
                text == "import$pkg.$name;" || text == "import$pkg.*;" ||
                    (text.startsWith("import$pkg.{") && Regex("[{,]$name[,}]").containsMatchIn(text))
            }
            if (covered) return null
            val anchor = imports.lastOrNull() ?: file.node.findChildByType(E.PACKAGE_STATEMENT)?.psi
            return if (anchor != null) {
                JuxRefactoringUtil.Edit(file, TextRange.from(anchor.textRange.endOffset, 0), "\nimport $pkg.$name;")
            } else {
                JuxRefactoringUtil.Edit(file, TextRange.from(0, 0), "import $pkg.$name;\n\n")
            }
        }
    }
}
