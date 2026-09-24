// Twin of 120_ref_alias_local.jux. Java has no `ref` binding, so a shared
// int lives in a one-element array here and a shared String in a
// one-element String array: Java shares an array the way Jux shares a `ref`
// cell. The algorithm and the printed values are identical, which is what
// the harness compares.
public class Main {
    static int aliasParam(int seed) {
        int[] r = new int[] { seed };
        r[0] = r[0] + 1;
        return r[0];
    }

    public static void main(String[] args) {
        int[] total = new int[] { 0 };
        int[] acc = total;
        acc[0] = 5;
        System.out.println(total[0]);
        System.out.println(acc[0]);
        total[0] = 9;
        System.out.println(acc[0]);

        String[] label = new String[] { "one" };
        String[] labelAlias = label;
        labelAlias[0] = "two";
        System.out.println(label[0]);
        System.out.println(labelAlias[0]);

        int[] fresh = new int[] { 3 };
        fresh[0] = 4;
        System.out.println(fresh[0]);

        System.out.println(aliasParam(5));
        int caller = 5;
        System.out.println(aliasParam(caller));
        System.out.println(caller);
    }
}
