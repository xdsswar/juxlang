public class Main {
    static class Money implements Comparable<Money> {
        private int cents;
        Money(int cents) { this.cents = cents; }
        int cents() { return this.cents; }

        Money plus(Money other) { return new Money(this.cents + other.cents()); }
        Money minus(Money other) { return new Money(this.cents - other.cents()); }
        Money times(int factor) { return new Money(this.cents * factor); }

        @Override public boolean equals(Object o) {
            return o instanceof Money && ((Money) o).cents == this.cents;
        }
        @Override public int hashCode() { return Integer.hashCode(this.cents); }
        @Override public int compareTo(Money other) { return this.cents - other.cents; }
        @Override public String toString() { return "$" + this.cents; }
    }

    static class Row {
        private java.util.List<Integer> slots = new java.util.ArrayList<>();
        Row() { slots.add(0); slots.add(0); slots.add(0); }
        int get(int i) { return slots.get(i); }
        void set(int i, int v) { slots.set(i, v); }
    }

    public static void main(String[] args) {
        Money a = new Money(150);
        Money b = new Money(275);

        System.out.println(a.plus(b));
        System.out.println(b.minus(a));
        System.out.println(a.times(3));
        System.out.println(a.equals(b));
        System.out.println(a.equals(new Money(150)));
        System.out.println(a.compareTo(b) < 0);
        System.out.println(b.compareTo(a) > 0);
        System.out.println(a.compareTo(new Money(150)) <= 0);

        Money acc = new Money(100);
        acc = acc.plus(new Money(50));
        System.out.println(acc);

        Row r = new Row();
        r.set(0, 7);
        r.set(2, 9);
        System.out.println(r.get(0));
        System.out.println(r.get(1));
        System.out.println(r.get(2));
        r.set(0, r.get(0) + r.get(2));
        System.out.println(r.get(0));
    }
}
