import java.util.Iterator;
import java.util.function.IntPredicate;
import java.util.function.IntUnaryOperator;

// Twin of 112_generator_pipelines.jux: each lazy stage is a hand-written
// Iterator that pulls from the stage before it.
public class Main {
    static class Range implements Iterator<Integer> {
        private int i;
        private final int to;

        Range(int from, int to) {
            this.i = from;
            this.to = to;
        }

        public boolean hasNext() { return i < to; }

        public Integer next() { return i++; }
    }

    static class Mapped implements Iterator<Integer> {
        private final Iterator<Integer> source;
        private final IntUnaryOperator f;

        Mapped(Iterator<Integer> source, IntUnaryOperator f) {
            this.source = source;
            this.f = f;
        }

        public boolean hasNext() { return source.hasNext(); }

        public Integer next() { return f.applyAsInt(source.next()); }
    }

    static class Filtered implements Iterator<Integer> {
        private final Iterator<Integer> source;
        private final IntPredicate keep;
        private Integer pending;

        Filtered(Iterator<Integer> source, IntPredicate keep) {
            this.source = source;
            this.keep = keep;
        }

        public boolean hasNext() {
            while (pending == null && source.hasNext()) {
                int x = source.next();
                if (keep.test(x)) pending = x;
            }
            return pending != null;
        }

        public Integer next() {
            hasNext();
            Integer v = pending;
            pending = null;
            return v;
        }
    }

    // The Jux stage pulls one value past the limit before it notices, so
    // this one does too.
    static class Take implements Iterator<Integer> {
        private final Iterator<Integer> source;
        private final int n;
        private int taken = 0;
        private boolean done = false;
        private Integer pending;

        Take(Iterator<Integer> source, int n) {
            this.source = source;
            this.n = n;
        }

        public boolean hasNext() {
            if (done) return false;
            if (pending != null) return true;
            if (!source.hasNext()) {
                done = true;
                return false;
            }
            int x = source.next();
            if (taken == n) {
                done = true;
                return false;
            }
            taken++;
            pending = x;
            return true;
        }

        public Integer next() {
            hasNext();
            Integer v = pending;
            pending = null;
            return v;
        }
    }

    static class Cycle<T> implements Iterator<T> {
        private final T a;
        private final T b;
        private final int total;
        private int emitted = 0;

        Cycle(T a, T b, int times) {
            this.a = a;
            this.b = b;
            this.total = times * 2;
        }

        public boolean hasNext() { return emitted < total; }

        public T next() { return emitted++ % 2 == 0 ? a : b; }
    }

    static class Probe {
        int pulls = 0;

        Iterator<Integer> numbers() {
            return new Iterator<Integer>() {
                private int n = 0;

                public boolean hasNext() { return true; }

                public Integer next() {
                    pulls++;
                    return n++;
                }
            };
        }
    }

    static class Checked implements Iterator<Integer> {
        private final Iterator<Integer> source;
        private final int limit;

        Checked(Iterator<Integer> source, int limit) {
            this.source = source;
            this.limit = limit;
        }

        public boolean hasNext() { return source.hasNext(); }

        public Integer next() {
            int x = source.next();
            if (x > limit) throw new IllegalArgumentException("too big: " + x);
            return x;
        }
    }

    public static void main(String[] args) {
        Iterator<Integer> squaresOfOdds =
            new Mapped(new Filtered(new Range(0, 10), x -> x % 2 == 1), x -> x * x);
        while (squaresOfOdds.hasNext()) {
            System.out.println("odd square " + squaresOfOdds.next());
        }

        Probe probe = new Probe();
        Iterator<Integer> taken = new Take(probe.numbers(), 3);
        while (taken.hasNext()) {
            System.out.println("taken " + taken.next());
        }
        System.out.println("pulls " + probe.pulls);

        Iterator<String> cyc = new Cycle<>("tick", "tock", 2);
        while (cyc.hasNext()) {
            System.out.println(cyc.next());
        }

        int total = 0;
        try {
            Iterator<Integer> ch = new Checked(new Range(1, 10), 4);
            while (ch.hasNext()) {
                total = total + ch.next();
            }
        } catch (IllegalArgumentException e) {
            System.out.println("stopped: " + e.getMessage());
        }
        System.out.println("total " + total);

        outer:
        for (Iterator<Integer> rows = new Range(0, 3); rows.hasNext(); ) {
            int row = rows.next();
            for (Iterator<Integer> cols = new Range(0, 3); cols.hasNext(); ) {
                int col = cols.next();
                if (col > row) continue outer;
                System.out.println("cell " + row + "," + col);
            }
        }
    }
}
