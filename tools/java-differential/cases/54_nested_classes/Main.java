public class Main {
    static class Outer {
        private int seed;
        Outer(int seed) { this.seed = seed; }

        static class Counter {
            private int n;
            Counter(int n) { this.n = n; }
            int next() {
                this.n = this.n + 1;
                return this.n;
            }
            int value() { return this.n; }
        }

        Counter counterFrom() {
            return new Counter(this.seed);
        }

        int seedTimes(int k) { return this.seed * k; }
    }

    public static void main(String[] args) {
        Outer o = new Outer(10);
        Outer.Counter c = o.counterFrom();
        System.out.println(c.next());
        System.out.println(c.next());
        System.out.println(c.value());
        System.out.println(o.seedTimes(3));

        Outer.Counter direct = new Outer.Counter(100);
        System.out.println(direct.next());
        System.out.println(direct.value());
        System.out.println(c.value());
    }
}
