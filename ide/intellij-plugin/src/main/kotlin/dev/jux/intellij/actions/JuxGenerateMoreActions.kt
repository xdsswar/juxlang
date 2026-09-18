package dev.jux.intellij.actions

import com.intellij.codeInsight.template.TemplateManager
import com.intellij.codeInsight.template.impl.ConstantNode
import com.intellij.icons.AllIcons
import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxMethodDeclaration
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxHierarchy
import dev.jux.intellij.resolve.JuxTypeIndex

/**
 * Generate a copy constructor, Java's "Copy Constructor": a constructor taking
 * another instance and copying the chosen fields from it. A class holds
 * shared references (§7.3), so a copied reference field still names the same
 * object, exactly as in Java.
 */
class JuxGenerateCopyConstructorAction : JuxGenerateAction() {
    override val includeComputed: Boolean = false
    override val choosesFields: Boolean = true
    override val chooserTitle: String = "Choose Fields to Copy"

    override fun isAvailableFor(type: JuxTypeDeclaration): Boolean =
        type.node.elementType === E.CLASS_DECLARATION || type.node.elementType === E.STRUCT_DECLARATION

    override fun build(type: JuxTypeDeclaration, className: String, fields: List<JuxField>): String {
        val self = selfTypeText(type, className)
        val copies = fields.joinToString("") { "        this.${it.name} = other.${it.name};\n" }
        val body = if (copies.isEmpty()) "" else "\n$copies    "
        return "\n    public $className($self other) {$body}\n"
    }
}

/**
 * Generate Delegate Methods, as Java's does: pick a field whose type is a
 * project type, pick some of that type's methods, and get one forwarding
 * method per choice. A generic field type's arguments are substituted into
 * the signatures (`Box<String> box` delegates `String get()`, not `T get()`).
 */
class JuxGenerateDelegateMethodsAction : JuxGenerateAction() {
    override val includeComputed: Boolean = false

    override fun isAvailableFor(type: JuxTypeDeclaration): Boolean =
        type.node.elementType === E.CLASS_DECLARATION || type.node.elementType === E.STRUCT_DECLARATION

    override fun build(type: JuxTypeDeclaration, className: String, fields: List<JuxField>): String? {
        val project = type.project
        val targets = fields.filter { targetType(type, it) != null }
        if (targets.isEmpty()) return null
        val field = JuxMemberChooser.choose(
            project, className, targets, "Select Target to Generate Delegates For",
            multiple = false, allowEmpty = false, icon = AllIcons.Nodes.Field,
        ) { "${it.name}: ${it.type}" }?.firstOrNull() ?: return null
        val target = targetType(type, field) ?: return null
        val candidates = delegatableMethods(type, target)
        if (candidates.isEmpty()) return null
        val chosen = JuxMemberChooser.choose(
            project, target.name ?: "", candidates, "Select Methods to Generate Delegates For",
            multiple = true, allowEmpty = false, icon = AllIcons.Nodes.Method,
        ) { JuxHierarchy.methodSignature(it) ?: (it as? JuxNamedElement)?.name ?: "?" } ?: return null
        val subst = substitution(type, field, target)
        return chosen.joinToString("") { delegate(it, field.name, subst) }
    }

    /** The project type a field holds, when it resolves to one. */
    private fun targetType(owner: JuxTypeDeclaration, field: JuxField): JuxTypeDeclaration? {
        val bare = field.type.substringBefore('<').substringAfterLast('.').trim()
        if (bare.isEmpty() || field.type.trimEnd().endsWith("]") || field.type.trimEnd().endsWith("?")) return null
        return JuxTypeIndex.findType(owner, bare)
    }

    /**
     * The methods of [target] (declared or inherited) [owner] can call and
     * does not already declare: not static, visible from [owner].
     */
    private fun delegatableMethods(owner: JuxTypeDeclaration, target: JuxTypeDeclaration): List<PsiElement> {
        val own = JuxHierarchy.directChildren(owner, E.METHOD_DECLARATION)
            .map { "${(it as? JuxNamedElement)?.name}/${JuxHierarchy.arity(it)}" }
            .toSet()
        return JuxHierarchy.allMembers(target).filter { m ->
            m.elementType === E.METHOD_DECLARATION &&
                !JuxHierarchy.hasModifier(m, "static") &&
                (JuxHierarchy.isInterface(JuxHierarchy.enclosingType(m) ?: target) ||
                    JuxHierarchy.memberVisibleFrom(m, owner)) &&
                "${(m as? JuxNamedElement)?.name}/${JuxHierarchy.arity(m)}" !in own
        }
    }

    /** The target type's parameters bound to the field type's arguments. */
    private fun substitution(owner: JuxTypeDeclaration, field: JuxField, target: JuxTypeDeclaration): Map<String, String> {
        val decl = JuxHierarchy.allMembersDeclaredIn(owner)
            .firstOrNull { (it as? JuxNamedElement)?.name == field.name }
        val ref = decl?.node?.findChildByType(E.TYPE_REFERENCE)?.psi ?: return emptyMap()
        return JuxHierarchy.typeParameterNames(target).zip(JuxHierarchy.typeArguments(ref)).toMap()
    }

    /** One forwarding method: the target's signature, calling through [field]. */
    private fun delegate(method: PsiElement, field: String, subst: Map<String, String>): String {
        val signature = JuxHierarchy.substituteTypeParams(JuxHierarchy.methodSignature(method) ?: "", subst)
        val name = (method as? JuxNamedElement)?.name ?: ""
        val call = "$field.$name(${JuxHierarchy.parameterNames(method).joinToString(", ")})"
        val returns = JuxHierarchy.returnTypeText(method)?.let { it != "void" } ?: false
        val statement = if (returns) "return $call;" else "$call;"
        return "\n    public $signature {\n        $statement\n    }\n"
    }
}

/**
 * Generate a test function or a test hook (§TS.1), Java's Generate > Test
 * Method / SetUp / TearDown. Jux tests are free `void` functions with no
 * parameters, so these are offered at the top of a file only, never in a
 * type (where the compiler would reject them, E0477). The name is a template
 * variable, ready to type over.
 */
abstract class JuxGenerateTestFunctionBase(
    private val annotation: String,
    private val defaultName: String,
) : AnAction() {
    override fun getActionUpdateThread() = ActionUpdateThread.BGT

    /** A hook appears once per file; tests as often as wanted. */
    protected open val oncePerFile: Boolean = true

    override fun update(e: AnActionEvent) {
        val file = e.getData(CommonDataKeys.PSI_FILE) as? JuxFile
        val editor = e.getData(CommonDataKeys.EDITOR)
        e.presentation.isEnabledAndVisible = file != null && editor != null &&
            topLevelAt(file, editor.caretModel.offset) &&
            !(oncePerFile && hasAnnotation(file, annotation))
    }

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val file = e.getData(CommonDataKeys.PSI_FILE) as? JuxFile ?: return
        val editor = e.getData(CommonDataKeys.EDITOR) ?: return
        val offset = topLevelInsertionOffset(file, editor.caretModel.offset)
        val name = uniqueName(file, defaultName)
        WriteCommandAction.runWriteCommandAction(project, "Generate", null, {
            val manager = TemplateManager.getInstance(project)
            val template = manager.createTemplate("", "")
            template.addTextSegment("\n@$annotation\nvoid ")
            template.addVariable("NAME", ConstantNode(name), ConstantNode(name), true)
            template.addTextSegment("() {\n    ")
            template.addEndVariable()
            template.addTextSegment("\n}\n")
            template.isToReformat = true
            editor.caretModel.moveToOffset(offset)
            manager.startTemplate(editor, template)
        })
    }

    companion object {
        /** True when [offset] is outside every type declaration. */
        fun topLevelAt(file: PsiFile, offset: Int): Boolean {
            val at = file.findElementAt(offset) ?: file.findElementAt(maxOf(0, offset - 1)) ?: return true
            return PsiTreeUtil.getParentOfType(at, JuxTypeDeclaration::class.java) == null
        }

        /**
         * Where a new top-level function goes: after the top-level declaration
         * holding the caret (so never inside a body), else at the caret.
         */
        fun topLevelInsertionOffset(file: PsiFile, caret: Int): Int {
            for (child in file.children) {
                val range = child.textRange
                if (child !is com.intellij.psi.PsiWhiteSpace && range.startOffset < caret && caret < range.endOffset) {
                    return range.endOffset
                }
            }
            return caret
        }

        /** Whether a free function in [file] carries `@name` (any case, §TS.1). */
        fun hasAnnotation(file: PsiFile, name: String): Boolean =
            PsiTreeUtil.findChildrenOfType(file, JuxMethodDeclaration::class.java).any { m ->
                m.parent is JuxFile && PsiTreeUtil.findChildrenOfType(m, PsiElement::class.java).any {
                    it.elementType === E.ANNOTATION &&
                        it.text.removePrefix("@").substringBefore('(').trim().equals(name, ignoreCase = true)
                }
            }

        /** [base], or `base2`, `base3`, ... when a function already has the name. */
        fun uniqueName(file: PsiFile, base: String): String {
            val taken = PsiTreeUtil.findChildrenOfType(file, JuxMethodDeclaration::class.java)
                .mapNotNull { it.name }
                .toSet()
            if (base !in taken) return base
            return generateSequence(2) { it + 1 }.map { "$base$it" }.first { it !in taken }
        }
    }
}

/** Generate a `@Test` function. */
class JuxGenerateTestFunctionAction : JuxGenerateTestFunctionBase("Test", "testName") {
    override val oncePerFile: Boolean = false
}

/** Generate the file's `@BeforeEach` hook, Java's "SetUp Method". */
class JuxGenerateBeforeEachAction : JuxGenerateTestFunctionBase("BeforeEach", "setUp")

/** Generate the file's `@AfterEach` hook, Java's "TearDown Method". */
class JuxGenerateAfterEachAction : JuxGenerateTestFunctionBase("AfterEach", "tearDown")
