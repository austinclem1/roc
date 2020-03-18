use crate::expr::Env;
use crate::expr::Expr;
use crate::expr::Pattern;
use roc_collections::all::{MutMap, MutSet};
use roc_module::ident::TagName;
use roc_module::symbol::Symbol;

use crate::layout::Builtin;
use crate::layout::Layout;

/// COMPILE CASES

type Label = u64;

/// Users of this module will mainly interact with this function. It takes
/// some normal branches and gives out a decision tree that has "labels" at all
/// the leafs and a dictionary that maps these "labels" to the code that should
/// run.
pub fn compile(raw_branches: Vec<(Pattern<'_>, u64)>) -> DecisionTree {
    let formatted = raw_branches
        .into_iter()
        .map(|(pattern, index)| Branch {
            goal: index,
            patterns: vec![(Path::Empty, pattern)],
        })
        .collect();

    to_decision_tree(formatted)
}

#[derive(Clone, Debug, PartialEq)]
pub enum DecisionTree<'a> {
    Match(Label),
    Decision {
        path: Path,
        edges: Vec<(Test<'a>, DecisionTree<'a>)>,
        default: Option<Box<DecisionTree<'a>>>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Test<'a> {
    IsCtor {
        tag_id: u8,
        tag_name: TagName,
        union: crate::pattern::Union,
        arguments: Vec<Pattern<'a>>,
    },
    IsInt(i64),
    // float patterns are stored as u64 so they are comparable/hashable
    IsFloat(u64),
    IsStr(Box<str>),
    IsBit(bool),
    IsByte {
        tag_id: u8,
        num_alts: usize,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Path {
    Index { index: u64, path: Box<Path> },
    Unbox(Box<Path>),
    Empty,
}

// ACTUALLY BUILD DECISION TREES

#[derive(Clone, Debug, PartialEq)]
struct Branch<'a> {
    goal: Label,
    patterns: Vec<(Path, Pattern<'a>)>,
}

fn to_decision_tree(raw_branches: Vec<Branch>) -> DecisionTree {
    let branches: Vec<_> = raw_branches.into_iter().map(flatten_patterns).collect();

    match check_for_match(&branches) {
        Some(goal) => DecisionTree::Match(goal),
        None => {
            // TODO remove clone
            let path = pick_path(branches.clone());

            let (edges, fallback) = gather_edges(branches, &path);

            let mut decision_edges: Vec<_> = edges
                .into_iter()
                .map(|(a, b)| (a, to_decision_tree(b)))
                .collect();

            match (decision_edges.split_last_mut(), fallback.split_last()) {
                (Some(((_tag, decision_tree), rest)), None) if rest.is_empty() => {
                    // TODO remove clone
                    decision_tree.clone()
                }
                (_, None) => DecisionTree::Decision {
                    path,
                    edges: decision_edges,
                    default: None,
                },
                (None, Some(_)) => to_decision_tree(fallback),
                _ => DecisionTree::Decision {
                    path,
                    edges: decision_edges,
                    default: Some(Box::new(to_decision_tree(fallback))),
                },
            }
        }
    }
}

fn is_complete(tests: &[Test]) -> bool {
    let length = tests.len();
    debug_assert!(length > 0);
    match tests.get(length - 1) {
        None => unreachable!("should never happen"),
        Some(v) => match v {
            Test::IsCtor { union, .. } => length == union.alternatives.len(),
            Test::IsByte { num_alts, .. } => length == *num_alts,
            Test::IsBit(_) => length == 2,
            Test::IsInt(_) => false,
            Test::IsFloat(_) => false,
            Test::IsStr(_) => false,
        },
    }
}

fn flatten_patterns(branch: Branch) -> Branch {
    let mut result = Vec::with_capacity(branch.patterns.len());

    for path_pattern in branch.patterns {
        flatten(path_pattern, &mut result);
    }
    Branch {
        goal: branch.goal,
        patterns: result,
    }
}

fn flatten<'a>(path_pattern: (Path, Pattern<'a>), path_patterns: &mut Vec<(Path, Pattern<'a>)>) {
    match &path_pattern.1 {
        Pattern::AppliedTag { union, .. } => {
            if union.alternatives.len() == 1 {
                //        case map dearg ctorArgs of
                //          [arg] ->
                //            flatten (Unbox path, arg) otherPathPatterns
                //
                //          args ->
                //            foldr flatten otherPathPatterns (subPositions path args)
                // subPositions :: Path -> [Can.Pattern] -> [(Path, Can.Pattern)]
                // subPositions path patterns =
                //   Index.indexedMap (\index pattern -> (Index index path, pattern)) patterns
                //
                //
                // dearg :: Can.PatternCtorArg -> Can.Pattern
                // dearg (Can.PatternCtorArg _ _ pattern) =
                //   pattern

                todo!()
            } else {
                path_patterns.push(path_pattern);
            }
        }

        _ => {
            path_patterns.push(path_pattern);
        }
    }
}

/// SUCCESSFULLY MATCH

/// If the first branch has no more "decision points" we can finally take that
/// path. If that is the case we give the resulting label and a mapping from free
/// variables to "how to get their value". So a pattern like (Just (x,_)) will give
/// us something like ("x" => value.0.0)
fn check_for_match(branches: &Vec<Branch>) -> Option<Label> {
    match branches.get(0) {
        Some(Branch { goal, patterns }) if patterns.iter().all(|(_, p)| !needs_tests(p)) => {
            Some(*goal)
        }
        _ => None,
    }
}

/// GATHER OUTGOING EDGES

fn gather_edges<'a>(
    branches: Vec<Branch<'a>>,
    path: &Path,
) -> (Vec<(Test<'a>, Vec<Branch<'a>>)>, Vec<Branch<'a>>) {
    // TODO remove clone
    let relevant_tests = tests_at_path(path, branches.clone());

    let check = is_complete(&relevant_tests);

    // TODO remove clone
    let all_edges = relevant_tests
        .into_iter()
        .map(|t| edges_for(path, branches.clone(), t))
        .collect();

    let fallbacks = if check {
        vec![]
    } else {
        branches
            .into_iter()
            .filter(|b| is_irrelevant_to(path, b))
            .collect()
    };

    (all_edges, fallbacks)
}

/// FIND RELEVANT TESTS

fn tests_at_path<'a>(selected_path: &Path, branches: Vec<Branch<'a>>) -> Vec<Test<'a>> {
    // NOTE the ordering of the result is important!

    let mut visited = MutSet::default();
    let mut unique = Vec::new();

    let all_tests = branches
        .into_iter()
        .filter_map(|b| test_at_path(selected_path, b));

    for test in all_tests {
        if !visited.contains(&test) {
            visited.insert(test.clone());
            unique.push(test);
        }
    }

    unique
}

fn test_at_path<'a>(selected_path: &Path, branch: Branch<'a>) -> Option<Test<'a>> {
    use Pattern::*;
    use Test::*;

    match branch
        .patterns
        .iter()
        .find(|(path, _)| path == selected_path)
    {
        None => None,
        Some((_, pattern)) => match pattern {
            Identifier(_)
            | RecordDestructure(_, _)
            | Underscore
            | Shadowed(_, _)
            | UnsupportedPattern(_) => None,

            AppliedTag {
                tag_name,
                tag_id,
                arguments,
                union,
                ..
            } => Some(IsCtor {
                tag_id: *tag_id,
                tag_name: tag_name.clone(),
                union: union.clone(),
                arguments: arguments.clone().into_iter().map(|v| v.0).collect(),
            }),
            BitLiteral(v) => Some(IsBit(*v)),
            EnumLiteral { tag_id, enum_size } => Some(IsByte {
                tag_id: *tag_id,
                num_alts: *enum_size as usize,
            }),
            IntLiteral(v) => Some(IsInt(*v)),
            FloatLiteral(v) => Some(IsFloat(*v)),
            StrLiteral(v) => Some(IsStr(v.clone())),
        },
    }
}

/// BUILD EDGES

fn edges_for<'a>(
    path: &Path,
    branches: Vec<Branch<'a>>,
    test: Test<'a>,
) -> (Test<'a>, Vec<Branch<'a>>) {
    let new_branches = branches
        .into_iter()
        .filter_map(|b| to_relevant_branch(&test, path, b))
        .collect();

    (test, new_branches)
}

fn to_relevant_branch<'a>(test: &Test<'a>, path: &Path, branch: Branch<'a>) -> Option<Branch<'a>> {
    use Pattern::*;
    use Test::*;

    // TODO remove clone
    match extract(path, branch.patterns.clone()) {
        Extract::NotFound => Some(branch),
        Extract::Found {
            mut start,
            found_pattern: pattern,
            end,
        } => match pattern {
            RecordDestructure(_, _)
            | Identifier(_)
            | Underscore
            | Shadowed(_, _)
            | UnsupportedPattern(_) => Some(branch),
            AppliedTag {
                union,
                tag_name,
                arguments,
                ..
            } => {
                let mut arguments: Vec<_> = arguments.into_iter().map(|v| v.0).collect();
                match test {
                    IsCtor {
                        tag_name: test_name,
                        ..
                    } if &tag_name == test_name => {
                        // TODO can't we unbox whenever there is just one alternative, even if
                        // there are multiple arguments?
                        if arguments.len() == 1 && union.alternatives.len() == 1 {
                            let arg = arguments.remove(0);
                            {
                                start.push((Path::Unbox(Box::new(path.clone())), arg));
                                start.extend(end);
                            }
                        } else {
                            let sub_positions =
                                arguments.into_iter().enumerate().map(|(index, pattern)| {
                                    (
                                        Path::Index {
                                            index: index as u64,
                                            path: Box::new(path.clone()),
                                        },
                                        pattern,
                                    )
                                });
                            start.extend(sub_positions);
                            start.extend(end);
                        }

                        Some(Branch {
                            goal: branch.goal,
                            patterns: start,
                        })
                    }
                    _ => None,
                }
                /*
                 *
                Can.PCtor _ _ (Can.Union _ _ numAlts _) name _ ctorArgs ->
                    case test of
                      IsCtor _ testName _ _ _ | name == testName ->
                        Just $ Branch goal $
                          case map dearg ctorArgs of
                            [arg] | numAlts == 1 ->
                              start ++ [(Unbox path, arg)] ++ end

                            args ->
                              start ++ subPositions path args ++ end

                      _ ->
                        Nothing
                      */
            }
            StrLiteral(string) => match test {
                IsStr(test_str) if string == *test_str => {
                    start.extend(end);
                    Some(Branch {
                        goal: branch.goal,
                        patterns: start,
                    })
                }
                _ => None,
            },

            IntLiteral(int) => match test {
                IsInt(is_int) if int == *is_int => {
                    start.extend(end);
                    Some(Branch {
                        goal: branch.goal,
                        patterns: start,
                    })
                }
                _ => None,
            },

            FloatLiteral(float) => match test {
                IsFloat(test_float) if float == *test_float => {
                    start.extend(end);
                    Some(Branch {
                        goal: branch.goal,
                        patterns: start,
                    })
                }
                _ => None,
            },

            BitLiteral(bit) => match test {
                IsBit(test_bit) if bit == *test_bit => {
                    start.extend(end);
                    Some(Branch {
                        goal: branch.goal,
                        patterns: start,
                    })
                }
                _ => None,
            },

            EnumLiteral { tag_id, .. } => match test {
                IsByte {
                    tag_id: test_id, ..
                } if tag_id == *test_id => {
                    start.extend(end);
                    Some(Branch {
                        goal: branch.goal,
                        patterns: start,
                    })
                }

                _ => None,
            },
        },
    }
}

enum Extract<'a> {
    NotFound,
    Found {
        start: Vec<(Path, Pattern<'a>)>,
        found_pattern: Pattern<'a>,
        end: Vec<(Path, Pattern<'a>)>,
    },
}

fn extract<'a>(selected_path: &Path, path_patterns: Vec<(Path, Pattern<'a>)>) -> Extract<'a> {
    let mut start = Vec::new();

    // TODO remove this clone
    let mut copy = path_patterns.clone();

    // TODO potential ordering problem
    for (index, current) in path_patterns.into_iter().enumerate() {
        if &current.0 == selected_path {
            return Extract::Found {
                start,
                found_pattern: current.1,
                end: {
                    copy.drain(0..=index);
                    copy
                },
            };
        } else {
            start.push(current);
        }
    }

    Extract::NotFound
}

/// FIND IRRELEVANT BRANCHES

fn is_irrelevant_to<'a>(selected_path: &Path, branch: &Branch<'a>) -> bool {
    match branch
        .patterns
        .iter()
        .find(|(path, _)| path == selected_path)
    {
        None => true,
        Some((_, pattern)) => !needs_tests(pattern),
    }
}

fn needs_tests<'a>(pattern: &Pattern<'a>) -> bool {
    use Pattern::*;

    match pattern {
        Identifier(_) | Underscore | Shadowed(_, _) | UnsupportedPattern(_) => false,

        RecordDestructure(_, _)
        | AppliedTag { .. }
        | BitLiteral(_)
        | EnumLiteral { .. }
        | IntLiteral(_)
        | FloatLiteral(_)
        | StrLiteral(_) => true,
    }
}

/// PICK A PATH

fn pick_path(branches: Vec<Branch>) -> Path {
    // TODO remove this clone
    let all_paths = branches
        .clone()
        .into_iter()
        .map(|v| v.patterns)
        .flatten()
        .filter_map(is_choice_path);

    let mut by_small_defaults = bests_by_small_defaults(&branches, all_paths);

    if by_small_defaults.len() == 1 {
        by_small_defaults.remove(0)
    } else {
        debug_assert!(!by_small_defaults.is_empty());
        let mut result = bests_by_small_branching_factor(&branches, by_small_defaults.into_iter());

        match result.pop() {
            None => unreachable!("bests_by will always return at least one value in the vec"),
            Some(path) => path,
        }
    }
}

fn is_choice_path(path_and_pattern: (Path, Pattern<'_>)) -> Option<Path> {
    let (path, pattern) = path_and_pattern;

    if needs_tests(&pattern) {
        Some(path)
    } else {
        None
    }
}

fn bests_by_small_branching_factor<I>(branches: &Vec<Branch>, mut all_paths: I) -> Vec<Path>
where
    I: Iterator<Item = Path>,
{
    match all_paths.next() {
        None => panic!("Cannot choose the best of zero paths. This should never happen."),
        Some(first_path) => {
            let mut min_weight = small_branching_factor(branches, &first_path);
            let mut min_paths = vec![first_path];

            for path in all_paths {
                let weight = small_branching_factor(branches, &path);

                use std::cmp::Ordering;
                match weight.cmp(&min_weight) {
                    Ordering::Equal => {
                        min_paths.push(path.clone());
                    }
                    Ordering::Less => {
                        min_weight = weight;
                        min_paths.clear();
                        min_paths.push(path);
                    }
                    Ordering::Greater => {}
                }
            }

            min_paths
        }
    }
}

fn bests_by_small_defaults<I>(branches: &Vec<Branch>, mut all_paths: I) -> Vec<Path>
where
    I: Iterator<Item = Path>,
{
    match all_paths.next() {
        None => panic!("Cannot choose the best of zero paths. This should never happen."),
        Some(first_path) => {
            let mut min_weight = small_defaults(branches, &first_path);
            let mut min_paths = vec![first_path];

            for path in all_paths {
                let weight = small_defaults(branches, &path);

                use std::cmp::Ordering;
                match weight.cmp(&min_weight) {
                    Ordering::Equal => {
                        min_paths.push(path.clone());
                    }
                    Ordering::Less => {
                        min_weight = weight;
                        min_paths.clear();
                        min_paths.push(path);
                    }
                    Ordering::Greater => {}
                }
            }

            min_paths
        }
    }
}

/// PATH PICKING HEURISTICS

fn small_defaults(branches: &Vec<Branch>, path: &Path) -> usize {
    branches
        .iter()
        .filter(|b| is_irrelevant_to(path, b))
        .map(|_| 1)
        .sum()
}

fn small_branching_factor(branches: &Vec<Branch>, path: &Path) -> usize {
    // TODO remove clone
    let (edges, fallback) = gather_edges(branches.to_vec(), path);

    edges.len() + (if fallback.is_empty() { 0 } else { 1 })
}

#[derive(Clone, Debug, PartialEq)]
enum Decider<'a, T> {
    Leaf(T),
    Chain {
        test_chain: Vec<(Path, Test<'a>)>,
        success: Box<Decider<'a, T>>,
        failure: Box<Decider<'a, T>>,
    },
    FanOut {
        path: Path,
        tests: Vec<(Test<'a>, Decider<'a, T>)>,
        fallback: Box<Decider<'a, T>>,
    },
}

#[derive(Clone, Debug, PartialEq)]
enum Choice<'a> {
    Inline(Expr<'a>),
    Jump(Label),
}

pub fn optimize_when<'a>(
    env: &mut Env<'a, '_>,
    cond_symbol: Symbol,
    cond_layout: Layout<'a>,
    ret_layout: Layout<'a>,
    opt_branches: Vec<(Pattern<'a>, Expr<'a>)>,
) -> Expr<'a> {
    let (patterns, _indexed_branches) = opt_branches
        .into_iter()
        .enumerate()
        .map(|(index, (pattern, branch))| ((pattern, index as u64), (index as u64, branch)))
        .unzip();

    let indexed_branches: Vec<(u64, Expr<'a>)> = _indexed_branches;

    let decision_tree = compile(patterns);
    let decider = tree_to_decider(decision_tree);
    let target_counts = count_targets(&decider);

    let mut choices = MutMap::default();
    let mut jumps = Vec::new();

    for (index, branch) in indexed_branches.into_iter() {
        let ((branch_index, choice), opt_jump) = create_choices(&target_counts, index, branch);

        if let Some(jump) = opt_jump {
            jumps.push(jump);
        }

        choices.insert(branch_index, choice);
    }

    let choice_decider = insert_choices(&choices, decider);

    let result = decide_to_branching(
        env,
        cond_symbol,
        cond_layout,
        ret_layout,
        choice_decider,
        &jumps,
    );

    // increase the jump counter by the number of jumps in this branching structure
    *env.jump_counter += jumps.len() as u64;

    result
}

fn path_to_expr<'a>(
    env: &mut Env<'a, '_>,
    symbol: Symbol,
    path: &Path,
    is_unwrapped: bool,
    field_layouts: Layout<'a>,
) -> Expr<'a> {
    match path {
        Path::Unbox(ref path) => path_to_expr(env, symbol, path, true, field_layouts),

        // TODO make this work with AccessAtIndex.
        // that already works for structs, but not for basic types for some reason
        //            Expr::AccessAtIndex {
        //                index: 0,
        //                field_layouts: env.arena.alloc([field_layouts]),
        //                expr: env.arena.alloc(Expr::Load(symbol)),
        //            },
        Path::Empty => Expr::Load(symbol),

        // TODO path contains a nested path. Traverse all the way
        Path::Index { index, .. } => Expr::AccessAtIndex {
            index: *index,
            field_layouts: env
                .arena
                .alloc([Layout::Builtin(Builtin::Byte(MutMap::default()))]),
            expr: env.arena.alloc(Expr::Load(symbol)),
            is_unwrapped,
        },
    }
}

fn decide_to_branching<'a>(
    env: &mut Env<'a, '_>,
    cond_symbol: Symbol,
    cond_layout: Layout<'a>,
    ret_layout: Layout<'a>,
    decider: Decider<Choice<'a>>,
    jumps: &Vec<(u64, Expr<'a>)>,
) -> Expr<'a> {
    use Choice::*;
    use Decider::*;

    let jump_count = *env.jump_counter;

    match decider {
        Leaf(Jump(label)) => Expr::Jump(label + jump_count),
        Leaf(Inline(expr)) => expr,
        Chain {
            test_chain,
            success,
            failure,
        } => {
            // generate a switch based on the test chain

            let mut tests = Vec::with_capacity(test_chain.len());

            for (path, test) in test_chain {
                match test {
                    Test::IsCtor { tag_id, union, .. } => {
                        let lhs = Expr::Int(tag_id as i64);

                        let field_layouts = env.arena.alloc([
                            Layout::Builtin(Builtin::Byte(MutMap::default())),
                            Layout::Builtin(Builtin::Int64),
                        ]);
                        let rhs = Expr::AccessAtIndex {
                            index: 0,
                            field_layouts,
                            expr: env.arena.alloc(Expr::Load(cond_symbol)),
                            is_unwrapped: union.alternatives.len() == 1,
                        };
                        // let rhs = Expr::Byte(tag_id);

                        let cond = env.arena.alloc(Expr::CallByName(
                            Symbol::INT_EQ_I64,
                            env.arena.alloc([
                                (lhs, Layout::Builtin(Builtin::Int64)),
                                (rhs, Layout::Builtin(Builtin::Int64)),
                            ]),
                        ));

                        tests.push(cond);
                    }
                    Test::IsInt(test_int) => {
                        let lhs = Expr::Int(test_int);
                        let rhs = path_to_expr(
                            env,
                            cond_symbol,
                            &path,
                            false,
                            Layout::Builtin(Builtin::Int64),
                        );

                        let cond = env.arena.alloc(Expr::CallByName(
                            Symbol::INT_EQ_I64,
                            env.arena.alloc([
                                (lhs, Layout::Builtin(Builtin::Int64)),
                                (rhs, Layout::Builtin(Builtin::Int64)),
                            ]),
                        ));

                        tests.push(cond);
                    }

                    Test::IsFloat(test_int) => {
                        // TODO maybe we can actually use i64 comparison here?
                        let test_float = f64::from_bits(test_int as u64);
                        let lhs = Expr::Float(test_float);
                        let rhs = path_to_expr(
                            env,
                            cond_symbol,
                            &path,
                            false,
                            Layout::Builtin(Builtin::Float64),
                        );

                        let cond = env.arena.alloc(Expr::CallByName(
                            Symbol::FLOAT_EQ,
                            env.arena.alloc([
                                (lhs, Layout::Builtin(Builtin::Float64)),
                                (rhs, Layout::Builtin(Builtin::Float64)),
                            ]),
                        ));

                        tests.push(cond);
                    }

                    Test::IsByte {
                        tag_id: test_byte,
                        // num_alts: _,
                        ..
                    } => {
                        let lhs = Expr::Byte(test_byte);
                        let rhs = path_to_expr(
                            env,
                            cond_symbol,
                            &path,
                            false,
                            Layout::Builtin(Builtin::Byte(MutMap::default())),
                        );

                        let fake = MutMap::default();

                        let cond = env.arena.alloc(Expr::CallByName(
                            Symbol::INT_EQ_I8,
                            env.arena.alloc([
                                (lhs, Layout::Builtin(Builtin::Byte(fake.clone()))),
                                (rhs, Layout::Builtin(Builtin::Byte(fake))),
                            ]),
                        ));

                        tests.push(cond);
                    }
                    _ => todo!(),
                }
            }

            let cond = tests.remove(0);

            let pass = env.arena.alloc(decide_to_branching(
                env,
                cond_symbol,
                cond_layout.clone(),
                ret_layout.clone(),
                *success,
                jumps,
            ));

            let fail = env.arena.alloc(decide_to_branching(
                env,
                cond_symbol,
                cond_layout.clone(),
                ret_layout.clone(),
                *failure,
                jumps,
            ));

            let cond_layout = Layout::Builtin(Builtin::Bool(
                TagName::Global("False".into()),
                TagName::Global("True".into()),
            ));

            Expr::Cond {
                cond,
                cond_layout,
                pass,
                fail,
                ret_layout,
            }
        }
        FanOut {
            path,
            tests,
            fallback,
        } => {
            let cond = env.arena.alloc(path_to_expr(
                env,
                cond_symbol,
                &path,
                false,
                cond_layout.clone(),
            ));

            let default_branch = env.arena.alloc(decide_to_branching(
                env,
                cond_symbol,
                cond_layout.clone(),
                ret_layout.clone(),
                *fallback,
                jumps,
            ));

            let mut branches = bumpalo::collections::Vec::with_capacity_in(tests.len(), env.arena);

            for (test, decider) in tests {
                let branch = decide_to_branching(
                    env,
                    cond_symbol,
                    cond_layout.clone(),
                    ret_layout.clone(),
                    decider,
                    jumps,
                );

                let tag = match test {
                    Test::IsInt(v) => v as u64,
                    Test::IsFloat(v) => v as u64,
                    Test::IsBit(v) => v as u64,
                    Test::IsByte { tag_id, .. } => tag_id as u64,
                    _ => todo!(),
                };

                branches.push((tag, branch));
            }

            // make a jump table based on the tests
            Expr::Switch {
                cond,
                cond_layout,
                // branches: &'a [(u64, Expr<'a>)],
                branches: branches.into_bump_slice(),
                // default_branch: &'a Expr<'a>,
                default_branch,
                ret_layout,
            }
        }
    }
}

/// TREE TO DECIDER
///
/// Decision trees may have some redundancies, so we convert them to a Decider
/// which has special constructs to avoid code duplication when possible.

fn tree_to_decider(tree: DecisionTree) -> Decider<u64> {
    use Decider::*;
    use DecisionTree::*;

    match tree {
        Match(target) => Leaf(target),

        Decision {
            path,
            mut edges,
            default,
        } => match default {
            None => match edges.len() {
                0 => panic!("compiler bug, somehow created an empty decision tree"),
                1 => {
                    let (_, sub_tree) = edges.remove(0);

                    tree_to_decider(sub_tree)
                }
                2 => {
                    let (_, failure_tree) = edges.remove(1);
                    let (test, success_tree) = edges.remove(0);

                    to_chain(path, test, success_tree, failure_tree)
                }

                _ => {
                    let fallback = edges.remove(edges.len() - 1).1;

                    let necessary_tests = edges
                        .into_iter()
                        .map(|(test, decider)| (test, tree_to_decider(decider)))
                        .collect();

                    FanOut {
                        path,
                        tests: necessary_tests,
                        fallback: Box::new(tree_to_decider(fallback)),
                    }
                }
            },

            Some(last) => match edges.len() {
                0 => tree_to_decider(*last),
                1 => {
                    let failure_tree = *last;
                    let (test, success_tree) = edges.remove(0);

                    to_chain(path, test, success_tree, failure_tree)
                }

                _ => {
                    let fallback = *last;

                    let necessary_tests = edges
                        .into_iter()
                        .map(|(test, decider)| (test, tree_to_decider(decider)))
                        .collect();

                    FanOut {
                        path,
                        tests: necessary_tests,
                        fallback: Box::new(tree_to_decider(fallback)),
                    }
                }
            },
        },
    }
}

fn to_chain<'a>(
    path: Path,
    test: Test<'a>,
    success_tree: DecisionTree<'a>,
    failure_tree: DecisionTree<'a>,
) -> Decider<'a, u64> {
    use Decider::*;

    let failure = tree_to_decider(failure_tree);

    match tree_to_decider(success_tree) {
        Chain {
            mut test_chain,
            success,
            failure: sub_failure,
        } if failure == *sub_failure => {
            test_chain.push((path, test));

            Chain {
                test_chain,
                success,
                failure: Box::new(failure),
            }
        }

        success => Chain {
            test_chain: vec![(path, test)],
            success: Box::new(success),
            failure: Box::new(failure),
        },
    }
}

/// INSERT CHOICES
///
/// If a target appears exactly once in a Decider, the corresponding expression
/// can be inlined. Whether things are inlined or jumps is called a "choice".

fn count_targets(decision_tree: &Decider<u64>) -> MutMap<u64, u64> {
    let mut result = MutMap::default();
    count_targets_help(decision_tree, &mut result);

    result
}

fn count_targets_help(decision_tree: &Decider<u64>, targets: &mut MutMap<u64, u64>) {
    use Decider::*;
    match decision_tree {
        Leaf(target) => match targets.get_mut(target) {
            None => {
                targets.insert(*target, 1);
            }
            Some(current) => {
                *current += 1;
            }
        },

        Chain {
            success, failure, ..
        } => {
            count_targets_help(success, targets);
            count_targets_help(failure, targets);
        }

        FanOut {
            tests, fallback, ..
        } => {
            count_targets_help(fallback, targets);

            for (_, decider) in tests {
                count_targets_help(decider, targets);
            }
        }
    }
}

fn create_choices<'a>(
    target_counts: &MutMap<u64, u64>,
    target: u64,
    branch: Expr<'a>,
) -> ((u64, Choice<'a>), Option<(u64, Expr<'a>)>) {
    match target_counts.get(&target) {
        None => unreachable!(
            "this should never happen: {:?} not in {:?}",
            target, target_counts
        ),
        Some(1) => ((target, Choice::Inline(branch)), None),
        Some(_) => ((target, Choice::Jump(target)), Some((target, branch))),
    }
}

fn insert_choices<'a>(
    choice_dict: &MutMap<u64, Choice<'a>>,
    decider: Decider<'a, u64>,
) -> Decider<'a, Choice<'a>> {
    use Decider::*;
    match decider {
        Leaf(target) => {
            // TODO remove clone
            // Only targes that appear once are Inline, so it's safe to remove them from the dict.
            Leaf(choice_dict[&target].clone())
        }

        Chain {
            test_chain,
            success,
            failure,
        } => Chain {
            test_chain,
            success: Box::new(insert_choices(choice_dict, *success)),
            failure: Box::new(insert_choices(choice_dict, *failure)),
        },

        FanOut {
            path,
            tests,
            fallback,
        } => FanOut {
            path,
            tests: tests
                .into_iter()
                .map(|(test, nested)| (test, insert_choices(choice_dict, nested)))
                .collect(),
            fallback: Box::new(insert_choices(choice_dict, *fallback)),
        },
    }
}

// Opt.FanOut path (map (second go) tests) (go fallback)
