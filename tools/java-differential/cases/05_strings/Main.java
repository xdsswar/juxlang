public class Main {
    public static void main(String[] args) {
        String s = "Hello, World";
        System.out.println(s.length());
        System.out.println(s.toUpperCase());
        System.out.println(s.toLowerCase());
        System.out.println(s.substring(0, 5));
        System.out.println(s.substring(7));
        System.out.println(s.indexOf("World"));
        System.out.println(s.indexOf("zzz"));
        System.out.println(s.contains("lo,"));
        System.out.println(s.startsWith("Hell"));
        System.out.println(s.endsWith("rld"));
        System.out.println(s.replace("World", "Jux"));
        System.out.println(s.trim());
        System.out.println("  pad  ".trim());
        System.out.println(s.charAt(1));
        System.out.println(s.isEmpty());
        System.out.println("".isEmpty());
        System.out.println("a" + 1 + 2);
        System.out.println(1 + 2 + "a");
        System.out.println(s.equals("Hello, World"));
        System.out.println("apple".compareTo("banana") < 0);
        System.out.println("banana".compareTo("apple") < 0);
    }
}
