package dev.jux.intellij.actions

import com.intellij.codeInsight.generation.ClassMember
import com.intellij.codeInsight.generation.MemberChooserObject
import com.intellij.codeInsight.generation.MemberChooserObjectBase
import com.intellij.icons.AllIcons
import com.intellij.ide.util.MemberChooser
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.project.Project
import org.jetbrains.annotations.TestOnly
import javax.swing.Icon

/**
 * One entry in a Generate chooser: a field or a method, grouped under the
 * type that declares it, as Java's member chooser shows them.
 */
class JuxChooserMember<T>(
    val value: T,
    text: String,
    icon: Icon,
    private val parent: MemberChooserObject,
) : MemberChooserObjectBase(text, icon), ClassMember {
    override fun getParentNodeDelegate(): MemberChooserObject = parent

    override fun equals(other: Any?): Boolean = other is JuxChooserMember<*> && other.value == value
    override fun hashCode(): Int = value.hashCode()
}

/**
 * The member choosers behind the Generate actions, Java's `MemberChooser`
 * dialog with everything preselected.
 *
 * In unit-test mode no dialog opens: the choice comes from [testChoice] when a
 * test set it, and is "everything offered" otherwise, which is what pressing
 * OK on the preselected dialog does.
 */
object JuxMemberChooser {

    /**
     * What a test answers for the next chooser: given the offered values,
     * return the chosen ones. Reset with `null`.
     */
    @TestOnly
    @JvmStatic
    var testChoice: ((List<Any?>) -> List<Any?>)? = null

    /** Choose among [fields] of [typeName]; `null` when cancelled. */
    fun chooseFields(project: Project, typeName: String, fields: List<JuxField>, title: String): List<JuxField>? =
        choose(project, typeName, fields, title, multiple = true, allowEmpty = true, icon = AllIcons.Nodes.Field) {
            "${it.name}: ${it.type}"
        }

    /**
     * The generic chooser: [values] of [typeName], labeled by [label]. With
     * [multiple] off the user picks one.
     */
    fun <T> choose(
        project: Project,
        typeName: String,
        values: List<T>,
        title: String,
        multiple: Boolean,
        allowEmpty: Boolean,
        icon: Icon,
        label: (T) -> String,
    ): List<T>? {
        if (ApplicationManager.getApplication().isUnitTestMode) {
            @Suppress("UNCHECKED_CAST")
            val chosen = testChoice?.invoke(values) as List<T>? ?: values
            return if (multiple) chosen else chosen.take(1)
        }
        val owner = MemberChooserObjectBase(typeName, AllIcons.Nodes.Class)
        val members = values.map { JuxChooserMember(it, label(it), icon, owner) }
        val chooser = MemberChooser(members.toTypedArray(), allowEmpty, multiple, project)
        chooser.title = title
        chooser.selectElements(if (multiple) members.toTypedArray() else members.take(1).toTypedArray())
        chooser.show()
        if (chooser.exitCode != MemberChooser.OK_EXIT_CODE) return null
        return chooser.selectedElements.orEmpty().map { it.value }
    }
}
