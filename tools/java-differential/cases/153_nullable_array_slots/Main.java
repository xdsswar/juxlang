public class Main {
    static class Holder {
        int[] data = null;
        String[] names;
        Holder() { this.names = null; }
        int[] get() { return data; }
        void set(int[] d) { this.data = d; }
    }

    static int[] maybe(boolean b) {
        if (b) { return new int[]{1, 2}; }
        return null;
    }

    static int count(int[] xs) {
        if (xs == null) { return -1; }
        return xs.length;
    }

    public static void main(String[] args) {
        int[] xs;
        xs = null;
        System.out.println(count(xs));
        int[] ys = new int[3];
        ys[0] = 7;
        System.out.println(count(ys) + " " + ys[0]);
        Integer[] slots = new Integer[2];
        System.out.println(slots[0] == null);
        Integer[] both = null;
        System.out.println(both == null);
        Holder h = new Holder();
        h.set(ys);
        h.get()[1] = 9;
        System.out.println(ys[1]);
        System.out.println(count(maybe(true)) + " " + count(maybe(false)));
        System.out.println(h.names == null);
    }
}
