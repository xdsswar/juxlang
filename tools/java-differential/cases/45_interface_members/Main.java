public class Main {
    interface Greeter {
        String PREFIX = ">> ";
        String name();
        default String greet() { return PREFIX + this.name(); }
        default String shout() { return this.greet() + "!"; }
    }
    interface Aged {
        default int age() { return 0; }
        default String label() { return "aged " + this.age(); }
    }
    static class Person implements Greeter, Aged {
        private String n;
        Person(String n) { this.n = n; }
        public String name() { return this.n; }
        public int age() { return 41; }
    }
    static class Anon implements Greeter, Aged {
        public String name() { return "anon"; }
    }

    public static void main(String[] args) {
        Person p = new Person("Ada");
        System.out.println(p.name());
        System.out.println(p.greet());
        System.out.println(p.shout());
        System.out.println(p.age());
        System.out.println(p.label());

        Anon a = new Anon();
        System.out.println(a.shout());
        System.out.println(a.label());

        System.out.println(Greeter.PREFIX + "direct");

        Greeter g = p;
        Aged ag = p;
        System.out.println(g.shout());
        System.out.println(ag.label());
    }
}
