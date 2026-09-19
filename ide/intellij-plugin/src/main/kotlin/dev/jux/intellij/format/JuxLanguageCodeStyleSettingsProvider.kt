package dev.jux.intellij.format

import com.intellij.application.options.IndentOptionsEditor
import com.intellij.application.options.SmartIndentOptionsEditor
import com.intellij.lang.Language
import com.intellij.psi.codeStyle.CodeStyleSettingsCustomizable
import com.intellij.psi.codeStyle.CommonCodeStyleSettings
import com.intellij.psi.codeStyle.LanguageCodeStyleSettingsProvider
import dev.jux.intellij.JuxLanguage

/**
 * The Settings | Editor | Code Style | **Jux** page: Tabs and Indents,
 * Spaces, Wrapping and Braces, Blank Lines and Imports, as Java's page has
 * them. Every option shown is one the formatter reads, under Java's name
 * (see [JuxSpacingRules]); Java options Jux's formatter does not implement
 * (wrapping long lines, alignment) are not shown.
 */
class JuxLanguageCodeStyleSettingsProvider : LanguageCodeStyleSettingsProvider() {
    override fun getLanguage(): Language = JuxLanguage

    /**
     * Java's defaults, except where Java's would rewrite code the Jux
     * formatter has always left alone: no minimum blank lines are added, and
     * a body written on one line is kept there.
     */
    override fun customizeDefaults(
        commonSettings: CommonCodeStyleSettings,
        indentOptions: CommonCodeStyleSettings.IndentOptions,
    ) {
        indentOptions.INDENT_SIZE = 4
        indentOptions.CONTINUATION_INDENT_SIZE = 8
        indentOptions.TAB_SIZE = 4
        indentOptions.USE_TAB_CHARACTER = false

        commonSettings.KEEP_SIMPLE_BLOCKS_IN_ONE_LINE = true
        commonSettings.KEEP_SIMPLE_METHODS_IN_ONE_LINE = true
        commonSettings.KEEP_SIMPLE_LAMBDAS_IN_ONE_LINE = true
        commonSettings.KEEP_SIMPLE_CLASSES_IN_ONE_LINE = true

        commonSettings.BLANK_LINES_AFTER_PACKAGE = 0
        commonSettings.BLANK_LINES_BEFORE_IMPORTS = 0
        commonSettings.BLANK_LINES_AFTER_IMPORTS = 0
        commonSettings.BLANK_LINES_AROUND_CLASS = 0
        commonSettings.BLANK_LINES_AROUND_FIELD = 0
        commonSettings.BLANK_LINES_AROUND_METHOD = 0
        commonSettings.BLANK_LINES_AROUND_FIELD_IN_INTERFACE = 0
        commonSettings.BLANK_LINES_AROUND_METHOD_IN_INTERFACE = 0
        commonSettings.BLANK_LINES_AFTER_CLASS_HEADER = 0
        commonSettings.BLANK_LINES_BEFORE_CLASS_END = 0
        commonSettings.BLANK_LINES_BEFORE_METHOD_BODY = 0
    }

    override fun getIndentOptionsEditor(): IndentOptionsEditor = SmartIndentOptionsEditor()

    /** Jux's own settings (the Imports tab), stored beside the common ones. */
    override fun createCustomSettings(settings: com.intellij.psi.codeStyle.CodeStyleSettings): com.intellij.psi.codeStyle.CustomCodeStyleSettings =
        JuxCodeStyleSettings(settings)

    /**
     * The page: the standard tabs (indents, spaces, wrapping, blank lines) plus
     * an Imports tab, as Java's page has.
     */
    override fun createConfigurable(
        baseSettings: com.intellij.psi.codeStyle.CodeStyleSettings,
        modelSettings: com.intellij.psi.codeStyle.CodeStyleSettings,
    ): com.intellij.psi.codeStyle.CodeStyleConfigurable =
        object : com.intellij.application.options.CodeStyleAbstractConfigurable(baseSettings, modelSettings, configurableDisplayName) {
            override fun createPanel(settings: com.intellij.psi.codeStyle.CodeStyleSettings): com.intellij.application.options.CodeStyleAbstractPanel =
                object : com.intellij.application.options.TabbedLanguageCodeStylePanel(JuxLanguage, currentSettings, settings) {
                    override fun initTabs(settings: com.intellij.psi.codeStyle.CodeStyleSettings) {
                        super.initTabs(settings)
                        addTab(JuxImportsCodeStylePanel(settings))
                    }
                }
        }

    override fun customizeSettings(consumer: CodeStyleSettingsCustomizable, settingsType: SettingsType) {
        when (settingsType) {
            SettingsType.SPACING_SETTINGS -> consumer.showStandardOptions(
                "SPACE_AROUND_ASSIGNMENT_OPERATORS",
                "SPACE_AROUND_LOGICAL_OPERATORS",
                "SPACE_AROUND_EQUALITY_OPERATORS",
                "SPACE_AROUND_RELATIONAL_OPERATORS",
                "SPACE_AROUND_ADDITIVE_OPERATORS",
                "SPACE_AROUND_MULTIPLICATIVE_OPERATORS",
                "SPACE_AROUND_SHIFT_OPERATORS",
                "SPACE_AROUND_BITWISE_OPERATORS",
                "SPACE_AFTER_COMMA",
            )
            SettingsType.BLANK_LINES_SETTINGS -> consumer.showStandardOptions(
                // Keep maximum blank lines
                "KEEP_BLANK_LINES_IN_DECLARATIONS",
                "KEEP_BLANK_LINES_IN_CODE",
                "KEEP_BLANK_LINES_BEFORE_RBRACE",
                // Minimum blank lines
                "BLANK_LINES_AFTER_PACKAGE",
                "BLANK_LINES_BEFORE_IMPORTS",
                "BLANK_LINES_AFTER_IMPORTS",
                "BLANK_LINES_AROUND_CLASS",
                "BLANK_LINES_AFTER_CLASS_HEADER",
                "BLANK_LINES_BEFORE_CLASS_END",
                "BLANK_LINES_AROUND_FIELD_IN_INTERFACE",
                "BLANK_LINES_AROUND_FIELD",
                "BLANK_LINES_AROUND_METHOD_IN_INTERFACE",
                "BLANK_LINES_AROUND_METHOD",
                "BLANK_LINES_BEFORE_METHOD_BODY",
            )
            SettingsType.WRAPPING_AND_BRACES_SETTINGS -> {
                consumer.showStandardOptions(
                    // Keep when reformatting
                    "KEEP_LINE_BREAKS",
                    "KEEP_FIRST_COLUMN_COMMENT",
                    "KEEP_SIMPLE_BLOCKS_IN_ONE_LINE",
                    "KEEP_SIMPLE_METHODS_IN_ONE_LINE",
                    "KEEP_SIMPLE_LAMBDAS_IN_ONE_LINE",
                    "KEEP_SIMPLE_CLASSES_IN_ONE_LINE",
                    // 'if()' / 'do ... while()' / 'try' statements
                    "ELSE_ON_NEW_LINE",
                    "WHILE_ON_NEW_LINE",
                    "CATCH_ON_NEW_LINE",
                    "FINALLY_ON_NEW_LINE",
                    // Force braces (JuxBraceEnforcer)
                    "IF_BRACE_FORCE",
                    "DOWHILE_BRACE_FORCE",
                    "WHILE_BRACE_FORCE",
                    "FOR_BRACE_FORCE",
                )
                // Braces placement, with the styles the formatter implements.
                val group = "Braces placement"
                val names = JuxCodeStyleSettings.BRACE_STYLE_NAMES
                val values = JuxCodeStyleSettings.BRACE_STYLES
                consumer.showCustomOption(JuxCodeStyleSettings::class.java, "CLASS_BRACE_STYLE", "In class declaration", group, names, values)
                consumer.showCustomOption(JuxCodeStyleSettings::class.java, "METHOD_BRACE_STYLE", "In method declaration", group, names, values)
                consumer.showCustomOption(JuxCodeStyleSettings::class.java, "LAMBDA_BRACE_STYLE", "In lambda declaration", group, names, values)
                consumer.showCustomOption(JuxCodeStyleSettings::class.java, "BRACE_STYLE", "Others", group, names, values)
            }
            else -> {}
        }
    }

    override fun getCodeSample(settingsType: SettingsType): String = when (settingsType) {
        SettingsType.WRAPPING_AND_BRACES_SETTINGS -> WRAPPING_SAMPLE
        SettingsType.BLANK_LINES_SETTINGS -> BLANK_LINES_SAMPLE
        else -> SAMPLE
    }

    private companion object {
        /** What the Wrapping and Braces options change: braces, cuddled keywords, one-liners. */
        val WRAPPING_SAMPLE = """
            public class Till implements Priced {
                private int total;

                public int Total { get; set; }

                public int get() { return total; }

                public void ring(int amount) throws Error {
            // commented out at the first column
                    if (amount > 0) {
                        total = total + amount;
                    } else {
                        throw new Error("no sale");
                    }
                    try {
                        print(total);
                    } catch (Error e) {
                        print("failed");
                    } finally {
                        print("done");
                    }
                    do {
                        amount = amount - 1;
                    } while (amount > 0);
                    var twice = (int x) -> { return x * 2; };
                    for (int i = 0; i < 3; i++) { print(i); }
                }
            }

            interface Priced { double price(); }
        """.trimIndent()

        /** What the Blank Lines options change: headers, members, bodies. */
        val BLANK_LINES_SAMPLE = """
            package shop;
            import jux.std.testing.*;
            import shop.model.Item;
            public class Cart {
                private int count;
                private double total;
                public Cart() {
                    count = 0;
                }
                public void add(Item item) {
                    count = count + 1;


                    total = total + item.price();
                }
                class Line {
                    int qty;
                }
            }
            interface Priced {
                int SCALE = 100;
                double price();
                String label();
            }
        """.trimIndent()

        // Exercises everything the exposed knobs change: operators, commas,
        // generics, fat-arrow bodies, switch arms, chains, lambdas, interp.
        val SAMPLE = """
            package com.example.demo;

            import rust.std.collections.Map;

            @Override
            public class Greeter<T extends Named & Sized> implements Named {
                private const int MAX = 10;
                private String name;
                public String label -> "greeter";

                public Greeter(String name) {
                    this.name = name;
                }

                public String greet(String who, int times) throws Error {
                    var msg = "Hello, " + who + '!';
                    var tagged = ${'$'}"greeting = ${'$'}{msg}";
                    if (who != null && MAX > times) {
                        return msg.trim().toUpperCase();
                    } else {
                        times = times * 2 + 1;
                    }
                    var kind = switch (times) {
                        case 0 -> "zero";
                        case 1 | 2 -> "few";
                        default -> { yield "many"; }
                    };
                    for (var i : 0..times) {
                        print(kind);
                    }
                    var f = (x) -> x * x;
                    return tagged;
                }
            }

            public enum Color { Red, Green, Blue }
        """.trimIndent()
    }
}
