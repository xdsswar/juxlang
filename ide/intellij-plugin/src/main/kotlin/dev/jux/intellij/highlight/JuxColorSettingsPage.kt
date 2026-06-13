package dev.jux.intellij.highlight

import com.intellij.openapi.editor.colors.TextAttributesKey
import com.intellij.openapi.fileTypes.SyntaxHighlighter
import com.intellij.openapi.options.colors.AttributesDescriptor
import com.intellij.openapi.options.colors.ColorDescriptor
import com.intellij.openapi.options.colors.ColorSettingsPage
import dev.jux.intellij.JuxIcons
import javax.swing.Icon

/**
 * Adds a **Jux** page under Settings → Editor → Color Scheme, so every token
 * category is listed and individually customizable, with a live preview.
 */
class JuxColorSettingsPage : ColorSettingsPage {
    override fun getIcon(): Icon = JuxIcons.FILE
    override fun getHighlighter(): SyntaxHighlighter = JuxSyntaxHighlighter()
    override fun getDemoText(): String = DEMO
    override fun getAdditionalHighlightingTagToDescriptorMap(): MutableMap<String, TextAttributesKey> = TAGS
    override fun getAttributeDescriptors(): Array<AttributesDescriptor> = DESCRIPTORS
    override fun getColorDescriptors(): Array<ColorDescriptor> = ColorDescriptor.EMPTY_ARRAY
    override fun getDisplayName(): String = "Jux"

    companion object {
        /**
         * Demo-only tags for the annotator-driven colours (the preview pane
         * has no annotator pass, so `<call>…</call>` markers stand in).
         */
        private val TAGS: MutableMap<String, TextAttributesKey> = hashMapOf(
            "decl" to JuxSyntaxHighlighter.CLASS_NAME,
            "method" to JuxSyntaxHighlighter.METHOD_DECLARATION,
            "field" to JuxSyntaxHighlighter.FIELD,
            "type" to JuxSyntaxHighlighter.TYPE,
            "call" to JuxSyntaxHighlighter.METHOD_CALL,
            "param" to JuxSyntaxHighlighter.PARAMETER,
            "local" to JuxSyntaxHighlighter.LOCAL_VARIABLE,
            "typeParam" to JuxSyntaxHighlighter.TYPE_PARAMETER,
            "enumConst" to JuxSyntaxHighlighter.ENUM_CONSTANT,
            "annotation" to JuxSyntaxHighlighter.ANNOTATION,
            "interp" to JuxSyntaxHighlighter.INTERPOLATION,
            "escape" to JuxSyntaxHighlighter.VALID_ESCAPE,
            "badEscape" to JuxSyntaxHighlighter.INVALID_ESCAPE,
            "nativeOp" to JuxSyntaxHighlighter.NATIVE_OPERATION,
        )

        private val DESCRIPTORS = arrayOf(
            AttributesDescriptor("Keyword", JuxSyntaxHighlighter.KEYWORD),
            AttributesDescriptor("Type//Primitive type", JuxSyntaxHighlighter.TYPE),
            AttributesDescriptor("Type//Class declaration name", JuxSyntaxHighlighter.CLASS_NAME),
            AttributesDescriptor("Type//Type parameter", JuxSyntaxHighlighter.TYPE_PARAMETER),
            AttributesDescriptor("Type//Method declaration name", JuxSyntaxHighlighter.METHOD_DECLARATION),
            AttributesDescriptor("Type//Field name", JuxSyntaxHighlighter.FIELD),
            AttributesDescriptor("References//Method call", JuxSyntaxHighlighter.METHOD_CALL),
            AttributesDescriptor("References//Parameter", JuxSyntaxHighlighter.PARAMETER),
            AttributesDescriptor("References//Local variable", JuxSyntaxHighlighter.LOCAL_VARIABLE),
            AttributesDescriptor("References//Enum constant", JuxSyntaxHighlighter.ENUM_CONSTANT),
            AttributesDescriptor("Property//Native operation (attach, bind, …)", JuxSyntaxHighlighter.NATIVE_OPERATION),
            AttributesDescriptor("String//Interpolation delimiter", JuxSyntaxHighlighter.INTERPOLATION),
            AttributesDescriptor("String//Valid escape sequence", JuxSyntaxHighlighter.VALID_ESCAPE),
            AttributesDescriptor("String//Invalid escape sequence", JuxSyntaxHighlighter.INVALID_ESCAPE),
            AttributesDescriptor("Constant", JuxSyntaxHighlighter.CONSTANT),
            AttributesDescriptor("String", JuxSyntaxHighlighter.STRING),
            AttributesDescriptor("Char", JuxSyntaxHighlighter.CHAR),
            AttributesDescriptor("Number", JuxSyntaxHighlighter.NUMBER),
            AttributesDescriptor("Comment//Line", JuxSyntaxHighlighter.LINE_COMMENT),
            AttributesDescriptor("Comment//Block", JuxSyntaxHighlighter.BLOCK_COMMENT),
            AttributesDescriptor("Comment//Doc", JuxSyntaxHighlighter.DOC_COMMENT),
            AttributesDescriptor("Annotation", JuxSyntaxHighlighter.ANNOTATION),
            AttributesDescriptor("Operator", JuxSyntaxHighlighter.OPERATOR),
            AttributesDescriptor("Braces and operators//Braces", JuxSyntaxHighlighter.BRACES),
            AttributesDescriptor("Braces and operators//Brackets", JuxSyntaxHighlighter.BRACKETS),
            AttributesDescriptor("Braces and operators//Parentheses", JuxSyntaxHighlighter.PARENS),
            AttributesDescriptor("Braces and operators//Semicolon", JuxSyntaxHighlighter.SEMICOLON),
            AttributesDescriptor("Braces and operators//Comma", JuxSyntaxHighlighter.COMMA),
            AttributesDescriptor("Braces and operators//Dot", JuxSyntaxHighlighter.DOT),
        )

        private val DEMO = """
            package com.example.demo;

            import rust.std.collections.Map;

            /** A demo class. */
            <annotation>@Override</annotation>
            public class <decl>Greeter</decl><<typeParam>T</typeParam>> implements <decl>Named</decl> {
                private const <type>int</type> <field>MAX</field> = 10;   // a constant
                private <type>String</type> <field>name</field>;

                public ref <type>String</type> <field>shared</field>;   // a ref field (§M.13)

                public <type>String</type> <method>greet</method>(ref <type>String</type> <param>who</param>) throws <decl>Error</decl> {
                    var <local>msg</local> = "Hello, <escape>\n</escape><badEscape>\q</badEscape>" + <param>who</param> + '!';
                    var <local>tagged</local> = ${'$'}"greeting = <interp>${'$'}{</interp><local>msg</local><interp>}</interp>";
                    var <local>kind</local> = typeof(<param>who</param>);   // compile-time type name
                    if (<param>who</param> != null && <field>MAX</field> > 0) {
                        return <call>format</call>(<local>tagged</local>);
                    }
                    return null;
                }
            }

            public enum <decl>Color</decl> { <enumConst>Red</enumConst>, <enumConst>Green</enumConst>, <enumConst>Blue</enumConst> }

            public class <decl>Form</decl> {
                public <type>String</type> <field>Name</field> { get; set; } = "";
                public <type>String</type> <field>Title</field> { get; private set; } = "";
                private final <type>observer</type><<type>String</type>> <field>nameObs</field> = (<param>old</param>, <param>now</param>) -> {
                    <call>print</call>(<param>now</param>);
                };

                public void <method>wire</method>(<decl>Form</decl> <param>other</param>) {
                    <field>Name</field>.<type>observers</type>.<nativeOp>attach</nativeOp>(<field>nameObs</field>);
                    <field>Name</field>.<type>observers</type>.<nativeOp>clear</nativeOp>;
                    <call>print</call>(<field>Name</field>.<type>observers</type>.<nativeOp>size</nativeOp>);
                    <field>Title</field>.<nativeOp>bind</nativeOp>(<param>other</param>.<field>Name</field>);
                    <field>Title</field>.<nativeOp>unbind</nativeOp>();
                }
            }
        """.trimIndent()
    }
}
