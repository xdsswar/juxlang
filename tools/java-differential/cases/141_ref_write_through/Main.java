// Java's spelling of the Jux twin: where Jux writes `ref int n`, Java passes a
// one-element array (or a holder object) to get write-through, because its
// parameters are always by value.
public class Main {
    static class Holder {
        int value;
        Holder(int value) { this.value = value; }
    }

    static void bump(int[] n) {
        n[0] += 1;
    }

    static void addAll(int[] acc, int[] xs) {
        for (int x : xs) {
            acc[0] += x;
        }
    }

    static void raise(Holder h, int by) {
        h.value += by;
    }

    static void bumpCopy(int n) {
        n += 1000;
    }

    public static void main(String[] args) {
        int[] counter = {0};
        bump(counter);
        bump(counter);
        System.out.println(counter[0]);

        // Jux's `ref int alias = counter;` aliases the same cell; in Java the
        // array reference IS the alias.
        int[] alias = counter;
        alias[0] += 10;
        System.out.println(counter[0]);
        System.out.println(alias[0]);

        addAll(counter, new int[]{1, 2, 3});
        System.out.println(counter[0]);

        bumpCopy(counter[0]);
        System.out.println(counter[0]);

        int plain = 5;
        // Jux passes the plain value to a `ref` parameter, which copies it;
        // Java cannot pass an `int` to `bump` at all, so it copies explicitly.
        int[] copyOfPlain = {plain};
        bump(copyOfPlain);
        System.out.println(plain);

        Holder holder = new Holder(7);
        raise(holder, 3);
        System.out.println(holder.value);

        String[] label = {"start"};
        String[] same = label;
        same[0] = "changed";
        System.out.println(label[0]);
        System.out.println(label[0].length());
    }
}
