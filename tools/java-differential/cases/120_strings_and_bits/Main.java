import java.util.Map;
import java.util.TreeMap;

public class Main {
    static String caesar(String text, int shift) {
        String out = "";
        for (int i = 0; i < text.length(); i++) {
            char c = text.charAt(i);
            if (c >= 'a' && c <= 'z') {
                out += (char) ('a' + ((c - 'a' + shift) % 26 + 26) % 26);
            } else if (c >= 'A' && c <= 'Z') {
                out += (char) ('A' + ((c - 'A' + shift) % 26 + 26) % 26);
            } else {
                out += c;
            }
        }
        return out;
    }

    static String runLength(String text) {
        if (text.length() == 0) { return ""; }
        String out = "";
        char prev = text.charAt(0);
        int count = 1;
        for (int i = 1; i < text.length(); i++) {
            char c = text.charAt(i);
            if (c == prev) {
                count++;
            } else {
                out += count + "" + prev;
                prev = c;
                count = 1;
            }
        }
        return out + count + prev;
    }

    static boolean palindrome(String text) {
        String letters = "";
        for (int i = 0; i < text.length(); i++) {
            char c = text.charAt(i);
            if (c >= 'A' && c <= 'Z') {
                letters += (char) (c + 32);
            } else if (c >= 'a' && c <= 'z') {
                letters += c;
            }
        }
        int lo = 0;
        int hi = letters.length() - 1;
        while (lo < hi) {
            if (letters.charAt(lo) != letters.charAt(hi)) { return false; }
            lo++;
            hi--;
        }
        return true;
    }

    static int popcount(long x) {
        int n = 0;
        long v = x;
        while (v != 0) {
            v = v & (v - 1);
            n++;
        }
        return n;
    }

    static boolean powerOfTwo(long x) {
        return x > 0 && (x & (x - 1)) == 0;
    }

    static long reverseLow16(long x) {
        long out = 0;
        for (int i = 0; i < 16; i++) {
            out = (out << 1) | ((x >> i) & 1);
        }
        return out;
    }

    public static void main(String[] args) {
        String secret = caesar("Hello, Jux World!", 3);
        System.out.println(secret);
        System.out.println(caesar(secret, -3));
        System.out.println(runLength("aaabccddddde"));
        System.out.println(palindrome("A man, a plan, a canal: Panama"));
        System.out.println(palindrome("Jux lang"));

        String[] words = "the quick brown fox jumps over the lazy dog the end".split(" ");
        TreeMap<String, Integer> counts = new TreeMap<>();
        for (String w : words) {
            counts.put(w, counts.getOrDefault(w, 0) + 1);
        }
        for (Map.Entry<String, Integer> e : counts.entrySet()) {
            if (e.getValue() > 1) {
                System.out.println(e.getKey() + " x" + e.getValue());
            }
        }

        long[] values = {0, 1, 6, 255, 1024, 65535, 123456789};
        for (long v : values) {
            System.out.println(v + ": bits=" + popcount(v) + " pow2=" + powerOfTwo(v) + " rev16=" + reverseLow16(v));
        }
        long flags = 0;
        flags |= 1L << 3;
        flags |= 1L << 40;
        flags ^= 1L << 3;
        System.out.println("flags=" + flags + " bit40=" + ((flags >> 40) & 1));
        System.out.println("mask=" + (0xFFL << 8) + " neg shift=" + (-16L >> 2));
    }
}
