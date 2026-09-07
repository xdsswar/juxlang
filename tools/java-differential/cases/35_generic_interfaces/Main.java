import java.util.ArrayList;

public class Main {
    interface Source<T> {
        T produce();
        default String announce() { return "produced " + this.produce(); }
    }
    interface Sink<T> {
        void accept(T value);
    }
    static class IntSource implements Source<Integer> {
        private int n;
        IntSource(int n) { this.n = n; }
        public Integer produce() { return this.n; }
    }
    static class EchoSource<T> implements Source<T> {
        private T v;
        EchoSource(T v) { this.v = v; }
        public T produce() { return this.v; }
    }
    static class Collector<T> implements Sink<T> {
        private ArrayList<T> seen;
        Collector() { this.seen = new ArrayList<>(); }
        public void accept(T value) { this.seen.add(value); }
        int count() { return this.seen.size(); }
        T at(int i) { return this.seen.get(i); }
    }

    public static void main(String[] args) {
        Source<Integer> a = new IntSource(4);
        System.out.println(a.produce());
        System.out.println(a.announce());

        Source<String> b = new EchoSource<>("x");
        System.out.println(b.produce());
        System.out.println(b.announce());

        Collector<String> c = new Collector<>();
        c.accept("p");
        c.accept("q");
        System.out.println(c.count());
        System.out.println(c.at(0));
        System.out.println(c.at(1));

        Collector<EchoSource<Integer>> nested = new Collector<>();
        nested.accept(new EchoSource<>(11));
        System.out.println(nested.count());
        System.out.println(nested.at(0).produce());
    }
}
