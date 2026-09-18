public class Main {
    record Range(int lo, int hi) {
        Range {
            if (lo > hi) {
                int t = lo;
                lo = hi;
                hi = t;
            }
        }

        Range(int single) {
            this(single, single);
        }

        public int length() { return hi - lo; }
    }

    record Name(String first, String last) {
        Name {
            if (first.equals("")) {
                throw new IllegalArgumentException("empty first name");
            }
        }

        Name(String only) {
            this(only, "?");
        }

        public String full() { return first + " " + last; }
    }

    static String show(Range r) {
        return r.lo() + ".." + r.hi() + " (" + r.length() + ")";
    }

    public static void main(String[] args) {
        System.out.println(show(new Range(1, 5)));
        System.out.println(show(new Range(9, 2)));
        System.out.println(show(new Range(4)));
        System.out.println(new Range(9, 2).equals(new Range(2, 9)));
        System.out.println(new Name("Ada", "Lovelace").full());
        System.out.println(new Name("Plato").full());
        try {
            new Name("", "x");
            System.out.println("accepted");
        } catch (IllegalArgumentException e) {
            System.out.println("rejected: " + e.getMessage());
        }
    }
}
