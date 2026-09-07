public class Main {
    static class Counter {
        static int made = 0;
        static final int LIMIT = 3;
        private int id;
        Counter() {
            made = made + 1;
            this.id = made;
        }
        int getId() { return this.id; }
    }
    static class Config {
        static final String NAME = "jux";
        static final String FULL = NAME + "-lang";
        static final int DOUBLE = Counter.LIMIT * 2;
    }

    public static void main(String[] args) {
        System.out.println(Counter.made);
        Counter a = new Counter();
        Counter b = new Counter();
        System.out.println(a.getId());
        System.out.println(b.getId());
        System.out.println(Counter.made);
        System.out.println(Counter.LIMIT);
        System.out.println(Config.NAME);
        System.out.println(Config.FULL);
        System.out.println(Config.DOUBLE);
    }
}
