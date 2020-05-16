<?php

namespace Titan\Http\Controllers\Admin;

use Carbon\Carbon;
use Illuminate\Http\JsonResponse;
use Illuminate\Http\RedirectResponse;
use Illuminate\Http\Request;
use Illuminate\Routing\Controller;
use Illuminate\View\View;
use Titan\Area;
use Titan\Character;
use Titan\Http\Requests\CreateUpdateCharacterRequest;
use Titan\Stat;

class CharacterController extends Controller
{
    /**
     * Display a listing of the resource.
     *
     * @return \Illuminate\Http\Response
     */
    public function index(): View
    {
        return view('titan::admin.characters.index');
    }

    /**
     * Show the form for creating a new resource.
     *
     * @return \Illuminate\Http\Response
     */
    public function create(): View
    {
        $areas = Area::all();
        $stats = Stat::all();

        return view('titan::admin.characters.create', compact('areas', 'stats'));
    }

    /**
     * Store a newly created resource in storage.
     *
     * @param \Illuminate\Http\Request $request
     * @return \Illuminate\Http\Response
     */
    public function store(CreateUpdateCharacterRequest $request): RedirectResponse
    {
        $character = new Character();

        $character->fill($request->only('display_name', 'area_id'));
        $character->user_id = $request->input('user_id');
        $character->last_move = new Carbon();
        $character->save();

        $character->seedStats();

        $stats = $request->input('stats');

        $character->stats->each(function($stat) use($stats) {
            if($stat->type->type === Stat::TYPE_BOOLEAN)
            {
                if(isset($stats[$stat->stat_id]))
                    $value = 1;
                else
                    $value = 0;
            }

            if(!isset($value))
                $value = $stats[$stat->stat_id];

            $stat->value = $value;
            $stat->save();
        });

        flash("Character has been created")->success();

        return redirect()->route('admin.characters.edit', $character);
    }

    /**
     * Display the specified resource.
     *
     * @param int $id
     * @return \Illuminate\Http\Response
     */
    public function show($id): View
    {

        return view('titan::admin.characters.show');
    }

    /**
     * Show the form for editing the specified resource.
     *
     * @param int $id
     * @return \Illuminate\Http\Response
     */
    public function edit(Character $character): View
    {
        $areas = Area::all();
        $stats = Stat::all();
        return view('titan::admin.characters.edit', compact('character', 'areas', 'stats'));
    }

    /**
     * Update the specified resource in storage.
     *
     * @param \Illuminate\Http\Request $request
     * @param int $id
     * @return \Illuminate\Http\Response
     */
    public function update(CreateUpdateCharacterRequest $request, Character $character): RedirectResponse
    {
        $character->fill($request->only('display_name', 'area_id'));
        $character->save();

        $stats = $request->input('stats');

        $character->stats->each(function($stat) use($stats) {
            if($stat->type->type === Stat::TYPE_BOOLEAN)
            {
                if(isset($stats[$stat->stat_id]))
                    $value = 1;
                else
                    $value = 0;
            }

            if(!isset($value))
                $value = $stats[$stat->stat_id];

            $stat->value = $value;
            $stat->save();
        });

        flash("Character has been updated")->success();

        return redirect()->back();
    }

    /**
     * Remove the specified resource from storage.
     *
     * @param int $id
     * @return \Illuminate\Http\Response
     */
    public function destroy(Character $character)
    {
        $character->stats()->delete();
        $character->delete();
        flash("Character has been deleted")->success();

        return response()->json(['ok']);
    }

    public function datatable(): JsonResponse
    {
        return datatables(Character::select())
            ->addColumn('action', function ($character) {
                $routeEdit = route('admin.characters.edit', $character);
                $routeDelete = route('admin.characters.destroy', $character);
                $buttons = '';
                $buttons .= '<a href="' . $routeEdit . '" class="btn btn-xs btn-primary mr-3"><i class="fas fa-pen fa-sm text-white-50"></i> Edit</a>';
                $buttons .= '<a href="' . $routeDelete . '" class="btn btn-xs btn-danger delete"><i class="fas fa-times-circle fa-sm text-white-50"></i> Delete</a>';
                return $buttons;
            })
            ->editColumn('last_move', function ($character) {
                if (!$character->last_move) {
                    return 'Never';
                }

                return (new Carbon($character->last_move))->diffForHumans();
            })->toJson();
    }
}
