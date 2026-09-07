public class Main {
    interface Named {
        default String name() { return "anon"; }
        default String tag() { return "[" + this.name() + "]"; }
    }
    interface Aged {
        default int age() { return 0; }
    }
    static abstract class Person implements Named, Aged {
        protected String who;
        Person(String who) { this.who = who; }
        public String name() { return this.who; }
    }
    static class Adult extends Person {
        Adult(String w) { super(w); }
        public int age() { return 30; }
    }
    static class Ghost implements Named {}

    public static void main(String[] args) {
        Named a = new Adult("Ada");
        System.out.println(a.name());
        System.out.println(a.tag());
        System.out.println(new Adult("Bo").age());
        Named g = new Ghost();
        System.out.println(g.name());
        System.out.println(g.tag());
    }
}
