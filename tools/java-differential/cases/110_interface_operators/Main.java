import java.util.ArrayList;

public class Main {
    interface Addable<T> {
        T plus(T other);
    }

    interface Scalable<T> {
        T times(long k);
    }

    static class Money implements Addable<Money>, Scalable<Money> {
        long cents;

        Money(long cents) {
            this.cents = cents;
        }

        public Money plus(Money other) {
            return new Money(cents + other.cents);
        }

        public Money times(long k) {
            return new Money(cents * k);
        }
    }

    static class Tally implements Addable<Tally> {
        int count;
        String last;

        Tally(int count, String last) {
            this.count = count;
            this.last = last;
        }

        public Tally plus(Tally other) {
            return new Tally(count + other.count, other.last);
        }
    }

    static <T extends Addable<T>> T sum(ArrayList<T> items, T zero) {
        T total = zero;
        for (T item : items) {
            total = total.plus(item);
        }
        return total;
    }

    static <T extends Addable<T> & Scalable<T>> T sumScaled(ArrayList<T> items, T zero, long k) {
        return sum(items, zero).times(k);
    }

    static <T extends Addable<T>> T twice(T x) {
        return x.plus(x);
    }

    public static void main(String[] args) {
        ArrayList<Money> prices = new ArrayList<>();
        prices.add(new Money(199));
        prices.add(new Money(1));
        prices.add(new Money(-50));
        System.out.println(sum(prices, new Money(0)).cents);
        System.out.println(sumScaled(prices, new Money(10), 3L).cents);
        System.out.println(twice(new Money(21)).cents);

        ArrayList<Tally> marks = new ArrayList<>();
        marks.add(new Tally(2, "a"));
        marks.add(new Tally(3, "b"));
        marks.add(new Tally(4, "c"));
        Tally t = sum(marks, new Tally(0, "none"));
        System.out.println(t.count);
        System.out.println(t.last);
        System.out.println(twice(t).count);

        Addable<Money> base = new Money(7);
        System.out.println(base.plus(new Money(3)).cents);
    }
}
