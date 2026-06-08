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
    override fun getAdditionalHighlightingTagToDescriptorMap(): MutableMap<String, TextAttributesKey>? = null
    override fun getAttributeDescriptors(): Array<AttributesDescriptor> = DESCRIPTORS
    override fun getColorDescriptors(): Array<ColorDescriptor> = ColorDescriptor.EMPTY_ARRAY
    override fun getDisplayName(): String = "Jux"

    companion object {
        private val DESCRIPTORS = arrayOf(
            AttributesDescriptor("Keyword", JuxSyntaxHighlighter.KEYWORD),
            AttributesDescriptor("Type//Primitive type", JuxSyntaxHighlighter.TYPE),
            AttributesDescriptor("Type//Class declaration name", JuxSyntaxHighlighter.CLASS_NAME),
            AttributesDescriptor("Type//Method declaration name", JuxSyntaxHighlighter.METHOD_DECLARATION),
            AttributesDescriptor("Type//Field name", JuxSyntaxHighlighter.FIELD),
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
            @Override
            public class Greeter<T> implements Named {
                private const int MAX = 10;        // a constant
                private String name;

                public String greet(String who) throws Error {
                    var msg = "Hello, " + who + '!';
                    if (who != null && MAX > 0) {
                        return msg;
                    }
                    return null;
                }
            }

            public enum Color { Red, Green, Blue }
        """.trimIndent()
    }
}
