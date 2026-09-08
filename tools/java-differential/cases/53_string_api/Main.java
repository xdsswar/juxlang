public class Main {
    public static void main(String[] args) {
        String s = "Hello, World";
        System.out.println(s.length());
        System.out.println(s.substring(7));
        System.out.println(s.substring(0, 5));
        System.out.println(s.toUpperCase());
        System.out.println(s.toLowerCase());
        System.out.println(s.indexOf("World"));
        System.out.println(s.contains("lo,"));
        System.out.println(s.startsWith("Hell"));
        System.out.println(s.endsWith("ld"));
        System.out.println(s.replace("World", "Jux"));
        System.out.println(s.isEmpty());
        System.out.println("  padded  ".trim());
        System.out.println("ab".repeat(3));
        System.out.println("".isEmpty());
        String joined = "";
        for (int i = 0; i < 3; i++) {
            joined = joined + i;
        }
        System.out.println(joined);
        System.out.println(s.charAt(0));
        System.out.println(s.indexOf("zzz"));
    }
}
