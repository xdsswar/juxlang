import java.util.ArrayList;

public class Main {
    static class TreeNode {
        String key;
        int count = 1;
        TreeNode left = null;
        TreeNode right = null;
        TreeNode(String key) { this.key = key; }
    }

    static class Tree {
        private TreeNode root = null;

        void add(String key) { root = insert(root, key); }

        private TreeNode insert(TreeNode n, String key) {
            if (n == null) { return new TreeNode(key); }
            TreeNode node = n;
            int c = key.compareTo(node.key);
            if (c < 0) {
                node.left = insert(node.left, key);
            } else if (c > 0) {
                node.right = insert(node.right, key);
            } else {
                node.count = node.count + 1;
            }
            return node;
        }

        int countOf(String key) {
            TreeNode cur = root;
            while (cur != null) {
                TreeNode n = cur;
                int c = key.compareTo(n.key);
                if (c == 0) { return n.count; }
                cur = c < 0 ? n.left : n.right;
            }
            return 0;
        }

        private void walk(TreeNode n, ArrayList<String> out) {
            if (n == null) { return; }
            walk(n.left, out);
            out.add(n.key + ":" + n.count);
            walk(n.right, out);
        }

        ArrayList<String> inOrder() {
            ArrayList<String> out = new ArrayList<>();
            walk(root, out);
            return out;
        }

        private int heightOf(TreeNode n) {
            if (n == null) { return 0; }
            int l = heightOf(n.left);
            int r = heightOf(n.right);
            return 1 + (l > r ? l : r);
        }

        int height() { return heightOf(root); }

        String min() {
            TreeNode n = root;
            while (n.left != null) { n = n.left; }
            return n.key;
        }

        String max() {
            TreeNode n = root;
            while (n.right != null) { n = n.right; }
            return n.key;
        }

        private int rangeCount(TreeNode n, String lo, String hi) {
            if (n == null) { return 0; }
            int here = (n.key.compareTo(lo) >= 0 && n.key.compareTo(hi) <= 0) ? 1 : 0;
            return here + rangeCount(n.left, lo, hi) + rangeCount(n.right, lo, hi);
        }

        int between(String lo, String hi) { return rangeCount(root, lo, hi); }
    }

    public static void main(String[] args) {
        Tree t = new Tree();
        String text = "the quick brown fox jumps over the lazy dog the end";
        for (String w : text.split(" ")) {
            t.add(w);
        }
        for (String line : t.inOrder()) {
            System.out.println(line);
        }
        System.out.println(t.countOf("the"));
        System.out.println(t.countOf("cat"));
        System.out.println(t.height());
        System.out.println(t.min() + " " + t.max());
        System.out.println(t.between("d", "l"));
    }
}
