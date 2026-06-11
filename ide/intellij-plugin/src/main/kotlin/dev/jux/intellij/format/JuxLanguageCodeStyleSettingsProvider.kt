package dev.jux.intellij.format

import com.intellij.application.options.IndentOptionsEditor
import com.intellij.application.options.SmartIndentOptionsEditor
import com.intellij.lang.Language
import com.intellij.psi.codeStyle.CodeStyleSettingsCustomizable
import com.intellij.psi.codeStyle.CommonCodeStyleSettings
import com.intellij.psi.codeStyle.LanguageCodeStyleSettingsProvider
import dev.jux.intellij.JuxLanguage

/**
 * The Settings | Editor | Code Style | **Jux** page. Common-settings only for
 * v1 — Jux's K&R style is fixed (braces same line), so the exposed knobs are
 * the ones the formatter actually reads: indents, operator spacing, comma
 * spacing, and blank-line keeps.
 */
class JuxLanguageCodeStyleSettingsProvider : LanguageCodeStyleSettingsProvider() {
    override fun getLanguage(): Language = JuxLanguage

    override fun customizeDefaults(
        commonSettings: CommonCodeStyleSettings,
        indentOptions: CommonCodeStyleSettings.IndentOptions,
    ) {
        indentOptions.INDENT_SIZE = 4
        indentOptions.CONTINUATION_INDENT_SIZE = 8
        indentOptions.TAB_SIZE = 4
        indentOptions.USE_TAB_CHARACTER = false
    }

    override fun getIndentOptionsEditor(): IndentOptionsEditor = SmartIndentOptionsEditor()

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
                "KEEP_BLANK_LINES_IN_DECLARATIONS",
                "KEEP_BLANK_LINES_IN_CODE",
            )
            SettingsType.WRAPPING_AND_BRACES_SETTINGS -> consumer.showStandardOptions(
                "KEEP_LINE_BREAKS",
            )
            else -> {}
        }
    }

    override fun getCodeSample(settingsType: SettingsType): String = SAMPLE

    private companion object {
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
